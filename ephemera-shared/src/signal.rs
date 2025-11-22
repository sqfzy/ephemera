use crate::Symbol;

/// 交易信号
#[derive(Debug, Clone, PartialEq)]
pub enum Signal {
    /// 买入信号
    Buy {
        symbol: Symbol,
        price: f64,
        size: f64,
        reason: String,
    },
    /// 卖出信号
    Sell {
        symbol: Symbol,
        price: f64,
        size: f64,
        reason: String,
    },
    /// 持有/无操作
    Hold,
}

impl Signal {
    pub fn buy(symbol: Symbol, price: f64, size: f64, reason: impl Into<String>) -> Self {
        Self::Buy {
            symbol,
            price,
            size,
            reason: reason.into(),
        }
    }

    pub fn sell(symbol: Symbol, price: f64, size: f64, reason: impl Into<String>) -> Self {
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
