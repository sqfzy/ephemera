use crate::Timestamp;
use crate::order::Side;
use bytestring::ByteString;
use futures::StreamExt;
use futures_util::Stream;
use serde::Deserialize;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use strum::EnumDiscriminants;

#[derive(Debug, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(MarketDataType), derive(Deserialize))]
pub enum DataEnum {
    Trade(TradeData),
    Candle(CandleData),
    Book(BookData),
}

#[derive(Debug)]
pub struct TradeData {
    /// 交易所分配的唯一交易ID
    pub trade_id: ByteString,

    /// 产品ID，例如 "BTC-USDT"。
    pub symbol: ByteString,

    /// 最新成交价。
    pub price: f64,

    /// 最新成交的数量。
    pub quantity: f64,

    /// 交易方向
    pub side: Side,

    /// 行情数据产生的时间，Unix时间戳的毫秒数格式。
    pub timestamp: Timestamp,
}

#[derive(Debug)]
pub struct CandleData {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub timestamp: Timestamp,
}

#[derive(Debug)]
pub struct BookData {
    /// (价格, 数量)
    pub bids: Vec<(f64, f64)>,
    /// (价格, 数量)
    pub asks: Vec<(f64, f64)>,
    pub timestamp: Timestamp,
}

// #[derive(
//     Debug,
//     PartialEq,
//     Eq,
//     Clone,
//     Copy,
//     Serialize,
//     Deserialize,
//     EnumString,
//     Display,
//     IntoStaticStr,
//     AsRefStr,
// )]
// pub enum CandleInterval {
//     // 基础时间粒度
//     #[strum(serialize = "1m")]
//     M1,
//     #[strum(serialize = "3m")]
//     M3,
//     #[strum(serialize = "5m")]
//     M5,
//     #[strum(serialize = "15m")]
//     M15,
//     #[strum(serialize = "30m")]
//     M30,
//     #[strum(serialize = "1H")]
//     H1,
//     #[strum(serialize = "2H")]
//     H2,
//     #[strum(serialize = "4H")]
//     H4,
//
//     // 香港时间开盘价 K 线
//     #[strum(serialize = "6H")]
//     H6,
//     #[strum(serialize = "12H")]
//     H12,
//     #[strum(serialize = "1D")]
//     D1,
//     #[strum(serialize = "2D")]
//     D2,
//     #[strum(serialize = "3D")]
//     D3,
//     #[strum(serialize = "1W")]
//     W1,
//     #[strum(serialize = "1M")]
//     Month1, // 使用 Month1 避免与 M1 (minute) 冲突
//     #[strum(serialize = "3M")]
//     Month3,
//
//     // UTC 时间开盘价 K 线
//     #[strum(serialize = "6Hutc")]
//     H6utc,
//     #[strum(serialize = "12Hutc")]
//     H12utc,
//     #[strum(serialize = "1Dutc")]
//     D1utc,
//     #[strum(serialize = "2Dutc")]
//     D2utc,
//     #[strum(serialize = "3Dutc")]
//     D3utc,
//     #[strum(serialize = "1Wutc")]
//     W1utc,
//     #[strum(serialize = "1Mutc")]
//     Month1utc,
//     #[strum(serialize = "3Mutc")]
//     Month3utc,
// }
