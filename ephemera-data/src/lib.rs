use rust_decimal::Decimal;

pub type Timestamp = u64;
pub type Symbol = bytestring::ByteString;
pub type IntervalSc = u64;

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

#[derive(Debug, strum::EnumDiscriminants)]
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

#[derive(Debug)]
pub struct TradeData {
    /// 交易所分配的唯一交易ID
    pub trade_id: u64,

    /// 产品ID，例如 "BTC-USDT"。
    pub symbol: Symbol,

    /// 行情数据产生的时间，Unix时间戳的毫秒数格式。
    pub timestamp: Timestamp,

    /// 最新成交价。
    pub price: Decimal,

    /// 最新成交的数量。
    pub quantity: Decimal,

    /// 交易方向
    pub side: Side,
}

#[derive(Debug)]
pub struct CandleData {
    pub symbol: Symbol,
    pub interval_sc: IntervalSc,
    pub open_timestamp: Timestamp,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
}

#[derive(Debug)]
pub struct BookData {
    pub symbol: Symbol,
    pub timestamp: Timestamp,
    /// (价格, 数量)
    pub bids: Vec<(Decimal, Decimal)>,
    /// (价格, 数量)
    pub asks: Vec<(Decimal, Decimal)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Buy,
    Sell,
}

impl std::str::FromStr for Side {
    type Err = Box<dyn std::error::Error>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "buy" => Ok(Side::Buy),
            "sell" => Ok(Side::Sell),
            _ => Err(format!("Invalid order side: '{s}'").into()),
        }
    }
}
