mod model;

use crate::utils::transform_raw_stream;
use async_stream::stream;
use bytestring::ByteString;
use ephemera_data::*;
use eyre::{ContextCompat, Result, ensure};
use futures::{SinkExt, Stream, StreamExt};
use http::{StatusCode, header::USER_AGENT};
use itertools::Itertools;
use model::*;
use rand::random;
use serde::de::DeserializeOwned;
use std::pin::Pin;
use tokio_websockets::Message;

pub const BINANCE_WS_BASE_URI: &str = "wss://stream.binance.com:443";
pub const BINANCE_WS_COMBINED_STREAM_BASE_URI: &str = "wss://stream.binance.com:443/stream";

const METHOD_SUBSCRIBE: ByteString = ByteString::from_static("SUBSCRIBE");

pub async fn binance_trade_data_stream(
    symbols: Vec<impl std::fmt::Display>,
) -> eyre::Result<impl Stream<Item = Result<TradeData>>> {
    let request = WsRequest {
        id: random(),
        method: METHOD_SUBSCRIBE,
        params: Some(symbols.into_iter().map(trade_stream_name).collect_vec()),
    };
    binance_raw_data_stream::<WsDataResponse<RawTradeData>>(request)
        .await
        .map(transform_raw_stream)
}

pub async fn binance_candle_data_stream(
    symbols: Vec<impl std::fmt::Display>,
    interval: BinanceCandleInterval,
) -> eyre::Result<impl Stream<Item = Result<CandleData>>> {
    let request = WsRequest {
        id: random(),
        method: METHOD_SUBSCRIBE,
        params: Some(
            symbols
                .into_iter()
                .map(|s| candle_stream_name(s, interval.clone()))
                .collect_vec(),
        ),
    };
    binance_raw_data_stream::<WsDataResponse<RawCandleData>>(request)
        .await
        .map(transform_raw_stream)
}

pub async fn binance_book_data_stream(
    symbols: Vec<impl std::fmt::Display>,
    channel: BinanceBookChannel,
) -> eyre::Result<impl Stream<Item = Result<BookData>>> {
    let request = WsRequest {
        id: random(),
        method: METHOD_SUBSCRIBE,
        params: Some(
            symbols
                .into_iter()
                .map(|s| book_stream_name(s, channel.clone()))
                .collect_vec(),
        ),
    };

    match channel {
        BinanceBookChannel::Incremental_1000ms
        | BinanceBookChannel::Incremental_100ms
        | BinanceBookChannel::OtherIncremental(_) => {
            binance_raw_data_stream::<WsDataResponse<RawBookData>>(request)
                .await
                .map(|stream| {
                    Box::pin(transform_raw_stream(stream))
                        as Pin<Box<dyn Stream<Item = Result<BookData>> + Send>>
                })
        }
        BinanceBookChannel::Depth5_1000ms
        | BinanceBookChannel::Depth5_100ms
        | BinanceBookChannel::Depth10_1000ms
        | BinanceBookChannel::Depth10_100ms
        | BinanceBookChannel::Depth20_1000ms
        | BinanceBookChannel::Depth20_100ms
        | BinanceBookChannel::OtherSnapshot(_) => {
            binance_raw_data_stream::<WsDataResponse<RawBookSnapshotData>>(request)
                .await
                .map(|stream| {
                    Box::pin(transform_raw_stream(stream))
                        as Pin<Box<dyn Stream<Item = Result<BookData>> + Send>>
                })
        }
    }
}

async fn binance_raw_data_stream<DR: DeserializeOwned + Send + 'static>(
    request: WsRequest,
) -> Result<Pin<Box<dyn Stream<Item = Result<DR, eyre::Error>> + Send>>, eyre::Error> {
    let params = if let Some(params) = &request.params
        && !params.is_empty()
    {
        params
    } else {
        panic!("At least one channel must be specified for subscription");
    };

    let stream_names = params.join("/");
    let end_point = format!("{BINANCE_WS_COMBINED_STREAM_BASE_URI}?streams={stream_names}");

    let (mut client, upgrade_resp) = tokio_websockets::ClientBuilder::new()
        .uri(&end_point)?
        .add_header(USER_AGENT, "ephemera".try_into()?)?
        .connect()
        .await?;

    ensure!(
        upgrade_resp.status() == StatusCode::SWITCHING_PROTOCOLS,
        "WebSocket connection failed: {}",
        upgrade_resp.status(),
    );

    client
        .send(Message::text(simd_json::serde::to_string(&request)?))
        .await?;

    // Expect a response like this:
    // {
    //   "id": 1,
    //   "status": 200,
    //   "result": null
    // }
    let resp = simd_json::from_slice::<Response<()>>(
        &mut client
            .next()
            .await
            .wrap_err("Failed to subscribe")??
            .as_payload()
            .to_vec(),
    )?;
    ensure!(
        matches!(resp.content, Content::Success { result: _ }),
        "Failed to subscribe with response:\n {resp:?}",
    );

    let stream = stream! {
        while let Some(msg) = client.next().await {
            let msg = msg?;

            // Return a pong response for ping messages to keep the connection alive.
            if msg.is_ping() {
                client.send(Message::pong(msg.into_payload())).await?;
                continue;
            }

            match simd_json::from_slice::<DR>(&mut msg.as_payload().to_vec()) {
                Ok(resp) => yield Ok(resp),
                Err(e) => yield Err(e.into()),

            }
        }
    };

    Ok(Box::pin(stream))
}

fn trade_stream_name(symbol: impl std::fmt::Display) -> StreamName {
    format!("{symbol}@trade").into()
}

fn candle_stream_name(
    symbol: impl std::fmt::Display,
    interval: BinanceCandleInterval,
) -> StreamName {
    format!("{symbol}@{interval}").into()
}

fn book_stream_name(symbol: impl std::fmt::Display, channel: BinanceBookChannel) -> StreamName {
    format!("{symbol}@{channel}").into()
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, strum::IntoStaticStr, strum::Display)]
pub enum BinanceCandleInterval {
    #[strum(serialize = "kline_1s")]
    Candle1s,
    #[strum(serialize = "kline_1m")]
    Candle1m,
    #[strum(serialize = "kline_3m")]
    Candle3m,
    #[strum(serialize = "kline_5m")]
    Candle5m,
    #[strum(serialize = "kline_15m")]
    Candle15m,
    #[strum(serialize = "kline_30m")]
    Candle30m,
    #[strum(serialize = "kline_1h")]
    Candle1h,
    #[strum(serialize = "kline_2h")]
    Candle2h,
    #[strum(serialize = "kline_4h")]
    Candle4h,
    #[strum(serialize = "kline_6h")]
    Candle6h,
    #[strum(serialize = "kline_8h")]
    Candle8h,
    #[strum(serialize = "kline_12h")]
    Candle12h,
    #[strum(serialize = "kline_1d")]
    Candle1d,
    #[strum(serialize = "kline_3d")]
    Candle3d,
    #[strum(serialize = "kline_1w")]
    Candle1w,
    #[strum(serialize = "kline_1M")]
    Candle1M,

    Other(String),
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, PartialEq, Eq, Hash, strum::IntoStaticStr, strum::Display)]
pub enum BinanceBookChannel {
    /// Incremental diff depth stream. Updates every 1000ms.
    #[strum(serialize = "depth")]
    Incremental_1000ms,

    /// Incremental diff depth stream. Updates every 100ms.
    #[strum(serialize = "depth@100ms")]
    Incremental_100ms,

    OtherIncremental(String),

    /// 5 level snapshot. Updates every 1000ms.
    #[strum(serialize = "depth5")]
    Depth5_1000ms,

    /// 5 level snapshot. Updates every 100ms.
    #[strum(serialize = "depth5@100ms")]
    Depth5_100ms,

    /// 10 level snapshot. Updates every 1000ms.
    #[strum(serialize = "depth10")]
    Depth10_1000ms,

    /// 10 level snapshot. Updates every 100ms.
    #[strum(serialize = "depth10@100ms")]
    Depth10_100ms,

    /// 20 level snapshot. Updates every 1000ms.
    #[strum(serialize = "depth20")]
    Depth20_1000ms,

    /// 20 level snapshot. Updates every 100ms.
    #[strum(serialize = "depth20@100ms")]
    Depth20_100ms,

    OtherSnapshot(ByteString),
}

// TODO: 需要代理
#[cfg(test)]
mod tests {
    use super::*;
    use ephemera_data::Symbol;

    const SYMBOLS: [Symbol; 2] = [
        Symbol::from_static("btcusdt"),
        Symbol::from_static("ethusdt"),
    ];
    const TEST_DATA_NUM: usize = 5;

    #[tokio::test]
    async fn test_binance_trade_data_stream() {
        binance_trade_data_stream(SYMBOLS.to_vec())
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
    async fn test_binance_candle_data_stream() {
        binance_candle_data_stream(SYMBOLS.to_vec(), BinanceCandleInterval::Candle1s)
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
    async fn test_binance_book_data_stream() {
        binance_book_data_stream(SYMBOLS.to_vec(), BinanceBookChannel::Incremental_100ms)
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
