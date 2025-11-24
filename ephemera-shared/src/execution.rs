use crate::Symbol;
use serde::{Deserialize, Serialize};

/// 订单方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderSide {
    Buy,
    Sell,
}

/// 订单类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderType {
    /// 市价单
    Market,
    /// 限价单
    Limit,
    /// 只做 maker（post-only）
    #[serde(rename = "post_only")]
    PostOnly,
    /// 立即成交或取消（FOK - Fill or Kill）
    #[serde(rename = "fok")]
    Fok,
    /// 立即成交并取消剩余（IOC - Immediate or Cancel）
    #[serde(rename = "ioc")]
    Ioc,
}

/// 订单状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderState {
    /// 订单已取消
    Canceled,
    /// 订单活跃
    Live,
    /// 订单部分成交
    #[serde(rename = "partially_filled")]
    PartiallyFilled,
    /// 订单完全成交
    Filled,
    /// 订单被拒绝
    Rejected,
}

/// 交易模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TradeMode {
    /// 现货
    Cash,
    /// 全仓
    Cross,
    /// 逐仓
    Isolated,
}

/// 带交易对的信号
///
/// 由于 Signal 不包含 symbol，在需要执行交易时需要将信号与交易对配对
#[derive(Debug, Clone, PartialEq)]
pub enum Signal {
    /// 买入信号
    Buy {
        symbol: Symbol,
        price: f64,
        size: f64,
    },
    /// 卖出信号
    Sell {
        symbol: Symbol,
        price: f64,
        size: f64,
    },
    /// 持有/无操作
    Hold,
}

impl Signal {
    pub fn buy(symbol: Symbol, price: f64, size: f64) -> Self {
        Self::Buy {
            symbol,
            price,
            size,
        }
    }

    pub fn sell(symbol: Symbol, price: f64, size: f64) -> Self {
        Self::Sell {
            symbol,
            price,
            size,
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
