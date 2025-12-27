use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::{IntervalSc, Symbol, TimestampMs};
use std::cmp::Ordering;

pub type BookSide = SmallVec<[(f64, f64); 20]>;

pub const CANDLE_INTERVAL_SEC1: IntervalSc = 1;
pub const CANDLE_INTERVAL_MIN1: IntervalSc = 60;
pub const CANDLE_INTERVAL_MIN3: IntervalSc = 180;
pub const CANDLE_INTERVAL_MIN5: IntervalSc = 300;
pub const CANDLE_INTERVAL_MIN15: IntervalSc = 900;
pub const CANDLE_INTERVAL_MIN30: IntervalSc = 1800;
pub const CANDLE_INTERVAL_H1: IntervalSc = 3600;
pub const CANDLE_INTERVAL_H2: IntervalSc = 7200;
pub const CANDLE_INTERVAL_H4: IntervalSc = 14400;
pub const CANDLE_INTERVAL_H6: IntervalSc = 21600;
pub const CANDLE_INTERVAL_H8: IntervalSc = 28800;
pub const CANDLE_INTERVAL_H12: IntervalSc = 43200;
pub const CANDLE_INTERVAL_D1: IntervalSc = 86400;
pub const CANDLE_INTERVAL_D3: IntervalSc = 259200;
pub const CANDLE_INTERVAL_WEEK1: IntervalSc = 604800;
pub const CANDLE_INTERVAL_MON1: IntervalSc = 2592000;
pub const CANDLE_INTERVAL_MON3: IntervalSc = 7776000;

#[derive(Debug, Clone, PartialEq, strum::EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(MarketDataType))]
pub enum MarketData {
    Trade(TradeData),
    Candle(CandleData),
    Book(BookData),
}

impl From<TradeData> for MarketData {
    fn from(data: TradeData) -> Self {
        MarketData::Trade(data)
    }
}

impl From<CandleData> for MarketData {
    fn from(data: CandleData) -> Self {
        MarketData::Candle(data)
    }
}

impl From<BookData> for MarketData {
    fn from(data: BookData) -> Self {
        MarketData::Book(data)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradeData {
    pub symbol: Symbol,
    pub timestamp_ms: TimestampMs,
    pub price: f64,
    pub quantity: f64,
    pub side: Side,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CandleData {
    pub symbol: Symbol,
    pub interval_sc: IntervalSc,
    pub open_timestamp_ms: TimestampMs,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

impl CandleData {
    pub(crate) fn new_with_trade(trade: &TradeData, interval_sc: IntervalSc) -> Self {
        Self {
            symbol: trade.symbol.clone(),
            interval_sc,
            open_timestamp_ms: trade.timestamp_ms - (trade.timestamp_ms % (interval_sc * 1000)),
            open: trade.price,
            high: trade.price,
            low: trade.price,
            close: trade.price,
            volume: trade.quantity,
        }
    }

    #[inline]
    pub(crate) fn unchecked_agg_with_trade(&mut self, trade: &TradeData) {
        self.high = self.high.max(trade.price);
        self.low = self.low.min(trade.price);
        self.close = trade.price;
        self.volume += trade.quantity;
    }

    /// # Error:
    ///
    /// - Retrun `DataError::MismatchedSymbol` if the symbol of the trade does not match the candle's symbol.
    /// - Return `DataError::UnexpectedTimestamp` if the timestamp of the trade is earlier than the candle's open timestamp.
    pub(crate) fn agg_with_trade(&mut self, trade: &TradeData) -> DataResult<()> {
        if self.symbol != trade.symbol {
            return Err(DataError::MismatchedSymbol {
                expected: self.symbol.clone(),
                found: trade.symbol.clone(),
            });
        }

        if trade.timestamp_ms <= self.open_timestamp_ms {
            return Err(DataError::timestamp_should_be_before(
                self.open_timestamp_ms,
                trade.timestamp_ms,
            ));
        }

        self.unchecked_agg_with_trade(trade);
        Ok(())
    }

    pub(crate) fn unchecked_agg_with_candle(&mut self, candle: &CandleData) {
        self.interval_sc += candle.interval_sc;
        self.high = self.high.max(candle.high);
        self.low = self.low.min(candle.low);
        self.close = candle.close;
        self.volume += candle.volume;
    }

    /// # Error
    ///
    /// 1. If target_interval is not multiple of first interval_sc.
    /// 2. If symbol or interval_sc mismatched.
    pub fn agg_with_candle(&mut self, candle: &CandleData) -> DataResult<()> {
        if candle.symbol != self.symbol {
            return Err(DataError::MismatchedSymbol {
                expected: self.symbol.clone(),
                found: candle.symbol.clone(),
            });
        }

        if candle.interval_sc != self.interval_sc {
            return Err(DataError::MismatchedInterval {
                expected: self.interval_sc,
                found: candle.interval_sc,
            });
        }

        if candle.open_timestamp_ms <= self.open_timestamp_ms {
            return Err(DataError::timestamp_should_be_after(
                self.open_timestamp_ms,
                candle.open_timestamp_ms,
            ));
        }

        self.unchecked_agg_with_candle(candle);
        Ok(())
    }

    pub fn from_trades(trades: &[TradeData], interval_sc: IntervalSc) -> DataResult<Option<Self>> {
        if trades.is_empty() {
            return Ok(None);
        }

        let first_trade = &trades[0];
        let mut candle = Self::new_with_trade(first_trade, interval_sc);

        for trade in trades.iter().skip(1) {
            candle.agg_with_trade(trade)?;
        }

        Ok(Some(candle))
    }
}

// PERF: 使用 Arc 避免频繁克隆或者使用数组
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct BookData {
    pub symbol: Symbol,
    pub timestamp: TimestampMs,
    /// (价格, 数量)
    pub bids: BookSide,
    /// (价格, 数量)
    pub asks: BookSide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumString, Serialize, Deserialize)]
#[strum(ascii_case_insensitive)]
pub enum Side {
    #[strum(serialize = "buy")]
    #[serde(alias = "Buy", alias = "BUY")]
    Buy,
    #[strum(serialize = "sell")]
    #[serde(alias = "Sell", alias = "SELL")]
    Sell,
}

pub type DataResult<T> = std::result::Result<T, DataError>;

#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("Expect interval {expected}, but found {found}.")]
    MismatchedInterval {
        expected: IntervalSc,
        found: IntervalSc,
    },

    // Interval 无法整除
    #[error("Interval {target} cannot be divided by {base}.")]
    UnDivisibleInterval {
        target: IntervalSc,
        base: IntervalSc,
    },

    #[error("Expect symbol {expected}, but found {found}.")]
    MismatchedSymbol { expected: Symbol, found: Symbol },

    #[error(
        "Expect timestamp to be {} {expected}, but found {found}",
        display_ordering(expect_order)
    )]
    UnexpectedTimestamp {
        expect_order: Ordering,
        expected: TimestampMs,
        found: TimestampMs,
    },

    #[error("Unexpect end of stream.")]
    UnexpectedStreamEof,
}

impl DataError {
    pub fn timestamp_should_be_after(expected: TimestampMs, found: TimestampMs) -> Self {
        Self::UnexpectedTimestamp {
            expect_order: Ordering::Greater,
            expected,
            found,
        }
    }

    pub fn timestamp_should_be_before(expected: TimestampMs, found: TimestampMs) -> Self {
        Self::UnexpectedTimestamp {
            expect_order: Ordering::Less,
            expected,
            found,
        }
    }

    pub fn timestamp_should_be_equal(expected: TimestampMs, found: TimestampMs) -> Self {
        Self::UnexpectedTimestamp {
            expect_order: Ordering::Equal,
            expected,
            found,
        }
    }
}

fn display_ordering(order: &Ordering) -> &'static str {
    match order {
        Ordering::Less => "less than",
        Ordering::Equal => "equal to",
        Ordering::Greater => "greater than",
    }
}
