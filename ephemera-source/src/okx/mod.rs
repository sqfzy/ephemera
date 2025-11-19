pub mod xdp;

mod model;

pub use model::WsOperation;

use crate::utils::{transform_raw_vec_stream, transform_raw_vec_stream_with};
use async_stream::stream;
use bytestring::ByteString;
use ephemera_shared::*;
use eyre::{ContextCompat, Result, ensure};
use futures::{SinkExt, Stream, StreamExt};
use http::{StatusCode, Uri};
use itertools::Itertools;
use model::*;
use serde::de::DeserializeOwned;
use std::{pin::Pin, str::FromStr};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use tokio_websockets::{Connector, Message};

pub const OKX_WS_HOST: &str = "ws.okx.com:8443";
pub const OKX_WS_PUBLICE_ENDPOINT: &str = "wss://ws.okx.com:8443/ws/v5/public";
pub const OKX_WS_BUSINESS_ENDPOINT: &str = "wss://ws.okx.com:8443/ws/v5/business";

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
#[strum(serialize_all = "camelCase")]
pub enum OkxCandleInterval {
    Candle3M,
    Candle1M,
    Candle1W,
    Candle1D,
    Candle2D,
    Candle3D,
    Candle5D,
    Candle12H,
    Candle6H,
    Candle4H,
    Candle2H,
    Candle1H,
    Candle30m,
    Candle15m,
    Candle5m,
    Candle3m,
    Candle1m,
    Candle1s,

    // UTC channels
    Candle3Mutc,
    Candle1Mutc,
    Candle1Wutc,
    Candle1Dutc,
    Candle2Dutc,
    Candle3Dutc,
    Candle5Dutc,
    Candle12Hutc,
    Candle6Hutc,

    Other(u64),
}

impl From<OkxCandleInterval> for u64 {
    fn from(val: OkxCandleInterval) -> Self {
        match val {
            OkxCandleInterval::Candle3M => CANDLE_INTERVAL_3MON,
            OkxCandleInterval::Candle1M => CANDLE_INTERVAL_1MON,
            OkxCandleInterval::Candle1W => CANDLE_INTERVAL_1W,
            OkxCandleInterval::Candle1D => CANDLE_INTERVAL_1D,
            OkxCandleInterval::Candle2D => CANDLE_INTERVAL_1D * 2,
            OkxCandleInterval::Candle3D => CANDLE_INTERVAL_3D,
            OkxCandleInterval::Candle5D => CANDLE_INTERVAL_1D * 5,
            OkxCandleInterval::Candle12H => CANDLE_INTERVAL_12H,
            OkxCandleInterval::Candle6H => CANDLE_INTERVAL_6H,
            OkxCandleInterval::Candle4H => CANDLE_INTERVAL_4H,
            OkxCandleInterval::Candle2H => CANDLE_INTERVAL_2H,
            OkxCandleInterval::Candle1H => CANDLE_INTERVAL_1H,
            OkxCandleInterval::Candle30m => CANDLE_INTERVAL_30M,
            OkxCandleInterval::Candle15m => CANDLE_INTERVAL_15M,
            OkxCandleInterval::Candle5m => CANDLE_INTERVAL_5M,
            OkxCandleInterval::Candle3m => CANDLE_INTERVAL_3M,
            OkxCandleInterval::Candle1m => CANDLE_INTERVAL_1M,
            OkxCandleInterval::Candle1s => CANDLE_INTERVAL_1S,
            OkxCandleInterval::Candle3Mutc => CANDLE_INTERVAL_3M,
            OkxCandleInterval::Candle1Mutc => CANDLE_INTERVAL_1MON,
            OkxCandleInterval::Candle1Wutc => CANDLE_INTERVAL_1W,
            OkxCandleInterval::Candle1Dutc => CANDLE_INTERVAL_1D,
            OkxCandleInterval::Candle2Dutc => CANDLE_INTERVAL_1D * 2,
            OkxCandleInterval::Candle3Dutc => CANDLE_INTERVAL_3D,
            OkxCandleInterval::Candle5Dutc => CANDLE_INTERVAL_1D * 5,
            OkxCandleInterval::Candle12Hutc => CANDLE_INTERVAL_12H,
            OkxCandleInterval::Candle6Hutc => CANDLE_INTERVAL_6H,
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
        assert_eq!(OkxCandleInterval::Candle1s.to_string(), "candle1s");
        assert_eq!(OkxCandleInterval::Candle12Hutc.to_string(), "candle12Hutc");
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
        okx_candle_data_stream(SYMBOLS.to_vec(), OkxCandleInterval::Candle1s)
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
