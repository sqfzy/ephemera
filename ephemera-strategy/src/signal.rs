use rust_decimal::Decimal;
use ephemera_data::Symbol;

/// 交易信号
#[derive(Debug, Clone, PartialEq)]
pub enum Signal {
    /// 买入信号
    Buy {
        symbol: Symbol,
        price: Decimal,
        size: Decimal,
        reason: String,
    },
    /// 卖出信号
    Sell {
        symbol: Symbol,
        price: Decimal,
        size: Decimal,
        reason: String,
    },
    /// 持有/无操作
    Hold,
}

impl Signal {
    pub fn buy(symbol: Symbol, price: Decimal, size: Decimal, reason: impl Into<String>) -> Self {
        Self::Buy {
            symbol,
            price,
            size,
            reason: reason.into(),
        }
    }

    pub fn sell(symbol: Symbol, price: Decimal, size: Decimal, reason: impl Into<String>) -> Self {
        Self::Sell {
            symbol,
            price,
            size,
            reason: reason.into(),
        }
    }

    pub fn is_buy(&self) -> bool {
        matches!(self, Signal::Buy { .. })
    }

    pub fn is_sell(&self) -> bool {
        matches!(self, Signal::Sell { .. })
    }

    pub fn is_hold(&self) -> bool {
        matches!(self, Signal::Hold)
    }
}
