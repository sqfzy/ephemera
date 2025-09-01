use rust_decimal::Decimal;
use crate::{DataResult, IntervalSc, Symbol, TimestampMs, error::DataError};

pub const CANDLE_INTERVAL_1S: IntervalSc = 1;
pub const CANDLE_INTERVAL_1M: IntervalSc = 60;
pub const CANDLE_INTERVAL_3M: IntervalSc = 180;
pub const CANDLE_INTERVAL_5M: IntervalSc = 300;
pub const CANDLE_INTERVAL_15M: IntervalSc = 900;
pub const CANDLE_INTERVAL_30M: IntervalSc = 1800;
pub const CANDLE_INTERVAL_1H: IntervalSc = 3600;
pub const CANDLE_INTERVAL_2H: IntervalSc = 7200;
pub const CANDLE_INTERVAL_4H: IntervalSc = 14400;
pub const CANDLE_INTERVAL_6H: IntervalSc = 21600;
pub const CANDLE_INTERVAL_8H: IntervalSc = 28800;
pub const CANDLE_INTERVAL_12H: IntervalSc = 43200;
pub const CANDLE_INTERVAL_1D: IntervalSc = 86400;
pub const CANDLE_INTERVAL_3D: IntervalSc = 259200;
pub const CANDLE_INTERVAL_1W: IntervalSc = 604800;
pub const CANDLE_INTERVAL_1MON: IntervalSc = 2592000;
pub const CANDLE_INTERVAL_3MON: IntervalSc = 7776000;

#[derive(Debug, Clone, PartialEq, Eq, Hash, strum::EnumDiscriminants)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TradeData {
    // /// 交易所分配的唯一交易ID
    // pub trade_id: u64,
    /// 产品ID，例如 "BTC-USDT"。
    pub symbol: Symbol,

    /// 行情数据产生的时间，Unix时间戳的毫秒数格式。
    pub timestamp_ms: TimestampMs,

    /// 最新成交价。
    pub price: Decimal,

    /// 最新成交的数量。
    pub quantity: Decimal,

    /// 交易方向
    pub side: Side,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct CandleData {
    pub symbol: Symbol,
    pub interval_sc: IntervalSc,
    pub open_timestamp_ms: TimestampMs,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct BookData {
    pub symbol: Symbol,
    pub timestamp: TimestampMs,
    /// (价格, 数量)
    pub bids: Vec<(Decimal, Decimal)>,
    /// (价格, 数量)
    pub asks: Vec<(Decimal, Decimal)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumString)]
#[strum(ascii_case_insensitive)]
pub enum Side {
    #[strum(serialize = "buy")]
    Buy,
    #[strum(serialize = "sell")]
    Sell,
}
