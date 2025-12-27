use crate::{
    okx::{OKX_WS_BUSINESS_ENDPOINT, OKX_WS_HOST, OKX_WS_PUBLICE_ENDPOINT, model::*},
    utils::{transform_raw_vec_stream, transform_raw_vec_stream_with},
};
use async_stream::stream;
use bytestring::ByteString;
use ephemera_shared::*;
use ephemera_xdp::async_stream::XdpTcpStream;
use eyre::{ContextCompat, Result, ensure};
use futures::{SinkExt, Stream, StreamExt};
use http::{StatusCode, Uri};
use itertools::Itertools;
use serde::de::DeserializeOwned;
use std::{pin::Pin, str::FromStr};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use tokio_websockets::{Connector, Message};

pub async fn okx_trade_data_stream(
    symbols: Vec<impl Into<ByteString>>,
) -> eyre::Result<impl Stream<Item = Result<TradeData>>> {
    let request = WsRequest {
        op: WsOperation::Subscribe,
        args: symbols
            .into_iter()
            .map(|inst_id| Arg::new(ByteString::from_static("trades"), inst_id.into()))
            .collect_vec(),
        id: None,
    };
    let stream = TcpStream::connect(OKX_WS_HOST).await?;
    okx_raw_data_stream::<WsDataResponse<RawTradeData>>(OKX_WS_PUBLICE_ENDPOINT, request, stream)
        .await
        .map(transform_raw_vec_stream)
}

pub async fn okx_candle_data_stream(
    symbols: Vec<impl Into<ByteString>>,
    interval: OkxCandleInterval,
) -> eyre::Result<impl Stream<Item = Result<CandleData>>> {
    let request = WsRequest {
        op: WsOperation::Subscribe,
        args: symbols
            .into_iter()
            .map(|inst_id| {
                Arg::new(
                    ByteString::from_static(interval.clone().into()),
                    inst_id.into(),
                )
            })
            .collect_vec(),
        id: None,
    };
    let stream = TcpStream::connect(OKX_WS_HOST).await?;
    okx_raw_data_stream::<WsDataResponse<RawCandleData>>(OKX_WS_BUSINESS_ENDPOINT, request, stream)
        .await
        .map(move |stream| {
            transform_raw_vec_stream_with(stream, move |resp| {
                convert_okx_candle_datas(resp, interval.clone().into())
            })
        })
}

pub async fn okx_book_data_stream(
    symbols: Vec<impl Into<ByteString>>,
    typ: OkxBookChannel,
) -> eyre::Result<impl Stream<Item = Result<BookData>>> {
    let request = WsRequest {
        op: WsOperation::Subscribe,
        args: symbols
            .into_iter()
            .map(|inst_id| Arg::new(ByteString::from_static(typ.clone().into()), inst_id.into()))
            .collect_vec(),
        id: None,
    };
    let stream = TcpStream::connect(OKX_WS_HOST).await?;
    okx_raw_data_stream::<WsDataResponse<OkxBookData>>(OKX_WS_PUBLICE_ENDPOINT, request, stream)
        .await
        .map(transform_raw_vec_stream)
}

pub async fn okx_xdp_trade_data_stream(
    symbols: Vec<impl Into<ByteString>>,
) -> eyre::Result<impl Stream<Item = Result<TradeData>>> {
    let request = WsRequest {
        op: WsOperation::Subscribe,
        args: symbols
            .into_iter()
            .map(|inst_id| Arg::new(ByteString::from_static("trades"), inst_id.into()))
            .collect_vec(),
        id: None,
    };
    let stream = XdpTcpStream::connect(OKX_WS_HOST).await?;
    okx_raw_data_stream::<WsDataResponse<RawTradeData>>(OKX_WS_PUBLICE_ENDPOINT, request, stream)
        .await
        .map(transform_raw_vec_stream)
}

pub async fn okx_xdp_candle_data_stream(
    symbols: Vec<impl Into<ByteString>>,
    interval: OkxCandleInterval,
) -> eyre::Result<impl Stream<Item = Result<CandleData>>> {
    let request = WsRequest {
        op: WsOperation::Subscribe,
        args: symbols
            .into_iter()
            .map(|inst_id| {
                Arg::new(
                    ByteString::from_static(interval.clone().into()),
                    inst_id.into(),
                )
            })
            .collect_vec(),
        id: None,
    };
    let stream = XdpTcpStream::connect(OKX_WS_HOST).await?;
    okx_raw_data_stream::<WsDataResponse<RawCandleData>>(OKX_WS_BUSINESS_ENDPOINT, request, stream)
        .await
        .map(move |stream| {
            transform_raw_vec_stream_with(stream, move |resp| {
                convert_okx_candle_datas(resp, interval.clone().into())
            })
        })
}

pub async fn okx_xdp_book_data_stream(
    symbols: Vec<impl Into<ByteString>>,
    typ: OkxBookChannel,
) -> eyre::Result<impl Stream<Item = Result<BookData>>> {
    let request = WsRequest {
        op: WsOperation::Subscribe,
        args: symbols
            .into_iter()
            .map(|inst_id| Arg::new(ByteString::from_static(typ.clone().into()), inst_id.into()))
            .collect_vec(),
        id: None,
    };
    let stream = XdpTcpStream::connect(OKX_WS_HOST).await?;
    okx_raw_data_stream::<WsDataResponse<OkxBookData>>(OKX_WS_PUBLICE_ENDPOINT, request, stream)
        .await
        .map(transform_raw_vec_stream)
}

// TODO: 返回sink和stream
async fn okx_raw_data_stream<DR: DeserializeOwned + Send + 'static>(
    end_point: &str,
    request: WsRequest,
    stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
) -> Result<Pin<Box<dyn Stream<Item = Result<DR, eyre::Error>> + Send>>, eyre::Error> {
    let channel_count = request.args.len();

    assert_ne!(
        channel_count, 0,
        "At least one channel must be specified for subscription"
    );

    let uri = Uri::from_str(end_point)?;
    let host = uri.host().expect("URI must have a host");

    let stream = if uri.scheme_str() == Some("wss") {
        Connector::new()?.wrap(host, stream).await?
    } else if uri.scheme_str() == Some("ws") {
        Connector::Plain.wrap(host, stream).await?
    } else {
        unreachable!()
    };

    let (mut client, upgrade_resp) = tokio_websockets::ClientBuilder::new()
        .uri(end_point)?
        .connect_on(stream)
        .await?;

    ensure!(
        upgrade_resp.status() == StatusCode::SWITCHING_PROTOCOLS,
        "WebSocket connection failed: {}",
        upgrade_resp.status(),
    );

    client
        .send(Message::text(simd_json::serde::to_string(&request)?))
        .await?;

    // Each channel subscription will get a response.
    let mut i = 0;
    while i < channel_count {
        // Expect a response like this:
        // {
        //   "event": "subscribe",
        //   "arg": {
        //     "channel": "trades",
        //     "instId": "BTC-USDT"
        //   },
        //   "id": "user_sub_01"
        // }
        let mut resp = client
            .next()
            .await
            .wrap_err("Failed to subscribe")??
            .as_payload()
            .to_vec();

        // WsResponse 并不总是连续的，有可能成功订阅第一个流之后，马上就在第二个 WsResponse
        // 之前收到数据，我们需要忽略它。
        if let Ok(resp) = simd_json::from_slice::<WsResponse>(&mut resp) {
            i += 1;
            ensure!(
                resp.event == WsOperation::Subscribe,
                "Failed to subscribe with response:\n {resp:?}",
            );
        }
    }

    let stream = stream! {
        while let Some(msg) = client.next().await {
            let msg = msg?;
            match simd_json::from_slice::<DR>(&mut msg.as_payload().to_vec()) {
                Ok(resp) => yield Ok(resp),
                Err(e) => yield Err(e.into()),

            }
        }
    };

    Ok(Box::pin(stream))
}

fn convert_okx_candle_datas(
    resp: WsDataResponse<RawCandleData>,
    interval_sc: u64,
) -> Result<Vec<CandleData>> {
    resp.data
        .into_iter()
        .take_while(|candle| matches!(candle.8.as_ref(), "1")) // 只取已完成的K线
        .map(|candle| {
            let open_timestamp = candle.0.parse()?;
            let open = candle.1.parse()?;
            let high = candle.2.parse()?;
            let low = candle.3.parse()?;
            let close = candle.4.parse()?;
            let volume = candle.5.parse()?;

            Ok(CandleData {
                symbol: resp.arg.inst_id.clone(),
                interval_sc,
                open_timestamp_ms: open_timestamp,
                open,
                high,
                low,
                close,
                volume,
            })
        })
        .try_collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, strum::IntoStaticStr, strum::Display)]
pub enum OkxCandleInterval {
    #[strum(serialize = "candle3M")]
    Mon3,
    #[strum(serialize = "candle1M")]
    Mon1,
    #[strum(serialize = "candle1W")]
    Week1,
    #[strum(serialize = "candle1D")]
    D1,
    #[strum(serialize = "candle2D")]
    D2,
    #[strum(serialize = "candle3D")]
    D3,
    #[strum(serialize = "candle5D")]
    D5,
    #[strum(serialize = "candle12H")]
    H12,
    #[strum(serialize = "candle6H")]
    H6,
    #[strum(serialize = "candle4H")]
    H4,
    #[strum(serialize = "candle2H")]
    H2,
    #[strum(serialize = "candle1H")]
    H1,
    #[strum(serialize = "candle30m")]
    Min30,
    #[strum(serialize = "candle15m")]
    Min15,
    #[strum(serialize = "candle5m")]
    Min5,
    #[strum(serialize = "candle3m")]
    Min3,
    #[strum(serialize = "candle1m")]
    Min1,
    #[strum(serialize = "candle1s")]
    Sec1,

    // UTC channels
    #[strum(serialize = "candle3Mutc")]
    UtcMon3,
    #[strum(serialize = "candle1Mutc")]
    UtcMon1,
    #[strum(serialize = "candle1Wutc")]
    UtcWeek1,
    #[strum(serialize = "candle1Dutc")]
    UtcD1,
    #[strum(serialize = "candle2Dutc")]
    UtcD2,
    #[strum(serialize = "candle3Dutc")]
    UtcD3,
    #[strum(serialize = "candle5Dutc")]
    UtcD5,
    #[strum(serialize = "candle12Hutc")]
    UtcH12,
    #[strum(serialize = "candle6Hutc")]
    UtcH6,

    Other(u64),
}

impl From<OkxCandleInterval> for u64 {
    fn from(val: OkxCandleInterval) -> Self {
        match val {
            OkxCandleInterval::Mon3 => CANDLE_INTERVAL_MON3,
            OkxCandleInterval::Mon1 => CANDLE_INTERVAL_MON1,
            OkxCandleInterval::Week1 => CANDLE_INTERVAL_WEEK1,
            OkxCandleInterval::D1 => CANDLE_INTERVAL_D1,
            OkxCandleInterval::D2 => CANDLE_INTERVAL_D1 * 2,
            OkxCandleInterval::D3 => CANDLE_INTERVAL_D3,
            OkxCandleInterval::D5 => CANDLE_INTERVAL_D1 * 5,
            OkxCandleInterval::H12 => CANDLE_INTERVAL_H12,
            OkxCandleInterval::H6 => CANDLE_INTERVAL_H6,
            OkxCandleInterval::H4 => CANDLE_INTERVAL_H4,
            OkxCandleInterval::H2 => CANDLE_INTERVAL_H2,
            OkxCandleInterval::H1 => CANDLE_INTERVAL_H1,
            OkxCandleInterval::Min30 => CANDLE_INTERVAL_MIN30,
            OkxCandleInterval::Min15 => CANDLE_INTERVAL_MIN15,
            OkxCandleInterval::Min5 => CANDLE_INTERVAL_MIN5,
            OkxCandleInterval::Min3 => CANDLE_INTERVAL_MIN3,
            OkxCandleInterval::Min1 => CANDLE_INTERVAL_MIN1,
            OkxCandleInterval::Sec1 => CANDLE_INTERVAL_SEC1,
            OkxCandleInterval::UtcMon3 => CANDLE_INTERVAL_MIN3,
            OkxCandleInterval::UtcMon1 => CANDLE_INTERVAL_MON1,
            OkxCandleInterval::UtcWeek1 => CANDLE_INTERVAL_WEEK1,
            OkxCandleInterval::UtcD1 => CANDLE_INTERVAL_D1,
            OkxCandleInterval::UtcD2 => CANDLE_INTERVAL_D1 * 2,
            OkxCandleInterval::UtcD3 => CANDLE_INTERVAL_D3,
            OkxCandleInterval::UtcD5 => CANDLE_INTERVAL_D1 * 5,
            OkxCandleInterval::UtcH12 => CANDLE_INTERVAL_H12,
            OkxCandleInterval::UtcH6 => CANDLE_INTERVAL_H6,
            OkxCandleInterval::Other(interval) => interval,
        }
    }
}

/// 只想在界面上显示最新价格？ -> BboTbt
/// 想做一个简单的盘口显示，不需要维护订单簿？ -> Books5
/// 做市或需要完整订单簿，但对延迟要求不高？ -> Books
/// 做高频交易，需要极低延迟但又不想处理整个订单簿？ -> Books50L2Tbt
/// 需要最强性能，既要完整订单簿又要最低延迟？ -> BooksL2Tbt
#[derive(Debug, Clone, PartialEq, Eq, Hash, strum::IntoStaticStr)]
pub enum OkxBookChannel {
    /// 5 depth levels snapshot, pushed every 100 ms.
    #[strum(serialize = "books5")]
    Books5,

    /// 400 depth levels incremental data, pushed every 100 ms.
    #[strum(serialize = "books")]
    Books,

    /// 1 depth level snapshot (Best Bid/Offer), pushed every 10 ms (tick-by-tick).
    #[strum(serialize = "bbo-tbt")]
    BboTbt,

    /// 400 depth levels incremental data, pushed every 10 ms (tick-by-tick).
    #[strum(serialize = "books-l2-tbt")]
    BooksL2Tbt,

    /// 50 depth levels incremental data, pushed every 10 ms (tick-by-tick).
    #[strum(serialize = "books50-l2-tbt")]
    Books50L2Tbt,

    Ohter(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use ephemera_shared::Symbol;

    const SYMBOLS: [Symbol; 2] = [
        Symbol::from_static("BTC-USDT"),
        Symbol::from_static("ETH-USDT"),
    ];
    const TEST_DATA_NUM: usize = 3;

    #[test]
    fn test_candle_interval_to_sting() {
        assert_eq!(OkxCandleInterval::Sec1.to_string(), "candle1s");
        assert_eq!(OkxCandleInterval::UtcH12.to_string(), "candle12Hutc");
    }

    #[tokio::test]
    async fn test_okx_trade_data_stream() {
        okx_trade_data_stream(SYMBOLS.to_vec())
            .await
            .unwrap()
            .take(TEST_DATA_NUM)
            .for_each(|res| {
                assert!(SYMBOLS.contains(&res.unwrap().symbol));
                std::future::ready(())
            })
            .await;
    }

    #[tokio::test]
    async fn test_okx_candle_data_stream() {
        okx_candle_data_stream(SYMBOLS.to_vec(), OkxCandleInterval::Sec1)
            .await
            .unwrap()
            .take(TEST_DATA_NUM)
            .for_each(|res| {
                assert!(SYMBOLS.contains(&res.unwrap().symbol));
                std::future::ready(())
            })
            .await;
    }

    #[tokio::test]
    async fn test_okx_book_data_stream() {
        okx_book_data_stream(SYMBOLS.to_vec(), OkxBookChannel::BboTbt)
            .await
            .unwrap()
            .take(TEST_DATA_NUM)
            .for_each(|res| {
                assert!(SYMBOLS.contains(&res.unwrap().symbol));
                std::future::ready(())
            })
            .await;
    }
}

#[cfg(test)]
#[serial_test::serial]
mod tests_xdp {
    use super::*;
    use ephemera_shared::Symbol;
    use std::sync::OnceLock;

    const SYMBOLS: [Symbol; 2] = [
        Symbol::from_static("BTC-USDT"),
        Symbol::from_static("ETH-USDT"),
    ];
    const TEST_DATA_NUM: usize = 3;

    fn setup() {
        static START: OnceLock<()> = OnceLock::new();

        START.get_or_init(|| {
            let level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());

            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::from_str(&level).unwrap())
                .init();
        });
    }

    #[tokio::test]
    async fn test_okx_xdp_trade_data_stream() {
        setup();
        okx_xdp_trade_data_stream(SYMBOLS.to_vec())
            .await
            .unwrap()
            .take(TEST_DATA_NUM)
            .for_each(|res| {
                assert!(SYMBOLS.contains(&res.unwrap().symbol));
                std::future::ready(())
            })
            .await;
    }

    #[tokio::test]
    async fn test_xdp_okx_candle_data_stream() {
        setup();
        okx_xdp_candle_data_stream(SYMBOLS.to_vec(), OkxCandleInterval::Sec1)
            .await
            .unwrap()
            .take(TEST_DATA_NUM)
            .for_each(|res| {
                assert!(SYMBOLS.contains(&res.unwrap().symbol));
                std::future::ready(())
            })
            .await;
    }

    #[tokio::test]
    async fn test_xdp_okx_book_data_stream() {
        setup();
        okx_xdp_book_data_stream(SYMBOLS.to_vec(), OkxBookChannel::BboTbt)
            .await
            .unwrap()
            .take(TEST_DATA_NUM)
            .for_each(|res| {
                assert!(SYMBOLS.contains(&res.unwrap().symbol));
                std::future::ready(())
            })
            .await;
    }
}
