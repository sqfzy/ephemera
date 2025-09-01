#![allow(dead_code)]

use ephemera_data::*;
use bytestring::ByteString;
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize};
use serde_with::{DisplayFromStr, serde_as};

/// eg. "btcusdt@aggTrade"
pub type StreamName = ByteString;

/// ```
/// {
///   "method": "SUBSCRIBE",
///   "params": [
///     "btcusdt@aggTrade",
///     "btcusdt@depth"
///   ],
///   "id": 1
/// }
/// ```
pub(super) type WsRequest = Request<Vec<StreamName>>;

#[derive(Debug, Serialize)]
pub(super) struct Request<T> {
    pub(super) id: u64,
    pub(super) method: ByteString,
    pub(super) params: Option<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Response<T> {
    pub(super) id: u64,
    pub(super) status: u16,
    #[serde(flatten)]
    pub(super) content: Content<T>,
    #[serde(default)]
    pub(super) rate_limits: Vec<RateLimit>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum Content<T> {
    Success { result: T },
    Error { error: ResponseError },
}

#[derive(Debug, Deserialize)]
pub(super) struct ResponseError {
    pub(super) code: i32,
    pub(super) msg: ByteString,
}

/// Example:
///
/// ```
/// {
///   "rateLimitType": "ORDERS",
///   "interval": "SECOND",
///   "intervalNum": 10,
///   "limit": 50,
///   "count": 12
/// }
/// ```
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RateLimit {
    pub(super) rate_limit_type: ByteString,
    #[serde(deserialize_with = "deserialize_interval")]
    pub(super) interval: IntervalSc,
    pub(super) interval_num: u32,
    pub(super) limit: u32,
    pub(super) count: u32,
}

/// Example:
///
/// ```
/// {
///   "stream": "btcusdt@trade",
///   "data": {
///     "e": "trade",
///     "E": 1672515788888,
///     "s": "BTCUSDT",
///     "t": 123456790,
///     "p": "23000.50",
///     "q": "0.002",
///     "b": 98767,
///     "a": 98768,
///     "T": 1672515788888,
///     "m": false,
///     "M": true
///   }
/// }
/// ```
#[derive(Debug, Deserialize)]
pub(super) struct WsDataResponse<RD> {
    pub(super) stream: StreamName,
    pub(super) data: RD,
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawTradeData {
    #[serde(rename = "E")]
    pub(super) event_time: TimestampMs,
    #[serde(rename = "s")]
    pub(super) symbol: ByteString,
    #[serde(rename = "t")]
    pub(super) trade_id: u64,

    #[serde_as(as = "DisplayFromStr")]
    #[serde(rename = "p")]
    pub(super) price: Decimal,

    #[serde_as(as = "DisplayFromStr")]
    #[serde(rename = "q")]
    pub(super) quantity: Decimal,

    #[serde(rename = "T")]
    pub(super) trade_time: TimestampMs,
    #[serde(rename = "m")]
    pub(super) is_buy: bool,
    #[serde(rename = "M")]
    pub(super) ignored: bool,
}

impl TryFrom<WsDataResponse<RawTradeData>> for TradeData {
    type Error = eyre::Error;

    fn try_from(value: WsDataResponse<RawTradeData>) -> Result<Self, Self::Error> {
        let side = if value.data.is_buy {
            Side::Buy
        } else {
            Side::Sell
        };
        Ok(Self {
            symbol: split_symbol_and_channel(value.stream)?.0,
            price: value.data.price,
            quantity: value.data.quantity,
            side,
            timestamp_ms: value.data.trade_time,
        })
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawCandleData {
    #[serde(rename = "E")]
    pub(super) event_time: TimestampMs,
    #[serde(rename = "s")]
    pub(super) symbol: ByteString,
    #[serde(rename = "k")]
    pub(super) kline: RawCandleDataInner,
}

impl TryFrom<WsDataResponse<RawCandleData>> for CandleData {
    type Error = eyre::Error;

    fn try_from(value: WsDataResponse<RawCandleData>) -> Result<Self, Self::Error> {
        let kline = value.data.kline;
        Ok(Self {
            symbol: split_symbol_and_channel(value.stream)?.0,
            interval_sc: kline.interval,
            open_timestamp_ms: kline.start_time,
            open: kline.open,
            high: kline.high,
            low: kline.low,
            close: kline.close,
            volume: kline.base_asset_volume,
        })
    }
}

#[serde_as]
#[derive(Debug, Deserialize)]
pub(super) struct RawCandleDataInner {
    #[serde(rename = "t")]
    pub(super) start_time: TimestampMs,
    #[serde(rename = "T")]
    pub(super) close_time: TimestampMs,
    #[serde(rename = "s")]
    pub(super) symbol: ByteString,
    #[serde(rename = "i", deserialize_with = "deserialize_interval")]
    pub(super) interval: IntervalSc,
    #[serde(rename = "f")]
    pub(super) first_trade_id: u64,
    #[serde(rename = "L")]
    pub(super) last_trade_id: u64,

    #[serde_as(as = "DisplayFromStr")]
    #[serde(rename = "o")]
    pub(super) open: Decimal,

    #[serde_as(as = "DisplayFromStr")]
    #[serde(rename = "c")]
    pub(super) close: Decimal,

    #[serde_as(as = "DisplayFromStr")]
    #[serde(rename = "h")]
    pub(super) high: Decimal,

    #[serde_as(as = "DisplayFromStr")]
    #[serde(rename = "l")]
    pub(super) low: Decimal,

    #[serde_as(as = "DisplayFromStr")]
    #[serde(rename = "v")]
    pub(super) base_asset_volume: Decimal,

    #[serde(rename = "n")]
    pub(super) number_of_trades: u64,
    #[serde(rename = "x")]
    pub(super) is_closed: bool,

    #[serde_as(as = "DisplayFromStr")]
    #[serde(rename = "q")]
    pub(super) quote_asset_volume: Decimal,

    #[serde_as(as = "DisplayFromStr")]
    #[serde(rename = "V")]
    pub(super) taker_buy_base_asset_volume: Decimal,

    #[serde_as(as = "DisplayFromStr")]
    #[serde(rename = "Q")]
    pub(super) taker_buy_quote_asset_volume: Decimal,

    pub(super) ignored: ByteString,
}

/// Represents an incremental update to the order book.
#[serde_as]
#[derive(Debug, Deserialize)]
pub(super) struct RawBookData {
    #[serde(rename = "E")]
    pub(super) event_time: TimestampMs,
    #[serde(rename = "s")]
    pub(super) symbol: ByteString,
    #[serde(rename = "U")]
    pub(super) first_update_id: u64,
    #[serde(rename = "u")]
    pub(super) final_update_id: u64,

    #[serde_as(as = "Vec<(DisplayFromStr, DisplayFromStr)>")]
    #[serde(rename = "b")]
    pub(super) bids: Vec<(Decimal, Decimal)>,

    #[serde_as(as = "Vec<(DisplayFromStr, DisplayFromStr)>")]
    #[serde(rename = "a")]
    pub(super) asks: Vec<(Decimal, Decimal)>,
}

impl TryFrom<WsDataResponse<RawBookData>> for BookData {
    type Error = eyre::Error;

    fn try_from(value: WsDataResponse<RawBookData>) -> Result<Self, Self::Error> {
        Ok(Self {
            symbol: split_symbol_and_channel(value.stream)?.0,
            timestamp: value.data.event_time,
            bids: value.data.bids,
            asks: value.data.asks,
        })
    }
}

/// Represents a partial book depth snapshot.
#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawBookSnapshotData {
    pub(super) last_update_id: u64,

    #[serde_as(as = "Vec<(DisplayFromStr, DisplayFromStr)>")]
    pub(super) bids: Vec<(Decimal, Decimal)>,

    #[serde_as(as = "Vec<(DisplayFromStr, DisplayFromStr)>")]
    pub(super) asks: Vec<(Decimal, Decimal)>,
}

impl TryFrom<WsDataResponse<RawBookSnapshotData>> for BookData {
    type Error = eyre::Error;

    fn try_from(value: WsDataResponse<RawBookSnapshotData>) -> Result<Self, Self::Error> {
        Ok(Self {
            symbol: split_symbol_and_channel(value.stream)?.0,
            // WARN: `last_update_id` is not the same as `timestamp`, but we use it as a timestamp here.
            timestamp: value.data.last_update_id,
            bids: value.data.bids,
            asks: value.data.asks,
        })
    }
}

#[inline]
fn split_symbol_and_channel(name: StreamName) -> eyre::Result<(ByteString, ByteString)> {
    let pos = name
        .find('@')
        .ok_or_else(|| eyre::eyre!("Invalid stream name format"))?;
    let (symbol, channel) = name.split_at(pos);
    let (_, channel) = channel.split_at(1); // Skip the '@' character
    Ok((symbol, channel))
}

pub fn deserialize_interval<'de, D>(deserializer: D) -> Result<IntervalSc, D::Error>
where
    D: Deserializer<'de>,
{
    let interval_str: &str = Deserialize::deserialize(deserializer)?;
    let interval = match interval_str {
        "1s" => CANDLE_INTERVAL_1S,
        "1m" => CANDLE_INTERVAL_1M,
        "3m" => CANDLE_INTERVAL_3M,
        "5m" => CANDLE_INTERVAL_5M,
        "15m" => CANDLE_INTERVAL_15M,
        "30m" => CANDLE_INTERVAL_30M,
        "1h" => CANDLE_INTERVAL_1H,
        "2h" => CANDLE_INTERVAL_2H,
        "4h" => CANDLE_INTERVAL_4H,
        "6h" => CANDLE_INTERVAL_6H,
        "8h" => CANDLE_INTERVAL_8H,
        "12h" => CANDLE_INTERVAL_12H,
        "1d" => CANDLE_INTERVAL_1D,
        "3d" => CANDLE_INTERVAL_3D,
        "1w" => CANDLE_INTERVAL_1W,
        "1M" => CANDLE_INTERVAL_1MON,
        _ => {
            return Err(serde::de::Error::custom(format!(
                "Unknown interval: {interval_str}"
            )));
        }
    };

    Ok(interval)
}
