use ephemera_shared::Symbol;
use std::collections::HashMap;

/// 风险管理器
///
/// 提供统一的风险控制和仓位计算功能，所有策略都应该使用此模块
/// 来确保风险管理的一致性和合规性。
#[derive(Debug, Clone)]
pub struct RiskManager {
    /// 单笔交易最大风险百分比（例如 0.02 = 2%）
    max_risk_per_trade: f64,

    /// 最大总风险敞口百分比（所有持仓的总风险，例如 0.06 = 6%）
    max_total_risk: f64,

    /// 单个品种最大持仓价值百分比（例如 0. 2 = 20%）
    max_position_size_pct: f64,

    /// 最大杠杆倍数（1.0 = 无杠杆）
    max_leverage: f64,

    /// 每个品种当前的风险敞口（已用风险）
    active_risks: HashMap<Symbol, f64>,
}

/// 仓位计算结果
#[derive(Debug, Clone, Copy)]
pub struct PositionSize {
    /// 建议的持仓数量
    pub size: f64,

    /// 入场价格
    pub entry_price: f64,

    /// 止损价格
    pub stop_loss: f64,

    /// 实际风险金额
    pub risk_amount: f64,

    /// 风险百分比
    pub risk_pct: f64,

    /// 持仓价值
    pub position_value: f64,
}

impl RiskManager {
    /// 创建新的风险管理器
    ///
    /// # 参数
    /// * `max_risk_per_trade` - 单笔交易最大风险（0. 01 = 1%）
    /// * `max_total_risk` - 总风险敞口上限（0.06 = 6%）
    /// * `max_position_size_pct` - 单个品种最大持仓占比（0.2 = 20%）
    /// * `max_leverage` - 最大杠杆倍数
    pub fn new(
        max_risk_per_trade: f64,
        max_total_risk: f64,
        max_position_size_pct: f64,
        max_leverage: f64,
    ) -> Result<Self, RiskError> {
        // 参数验证
        if max_risk_per_trade <= 0.0 || max_risk_per_trade > 0.1 {
            return Err(RiskError::InvalidParameter(
                "max_risk_per_trade should be between 0 and 0.1 (10%)".to_string(),
            ));
        }

        if max_total_risk <= 0.0 || max_total_risk > 0.5 {
            return Err(RiskError::InvalidParameter(
                "max_total_risk should be between 0 and 0.5 (50%)".to_string(),
            ));
        }

        if max_position_size_pct <= 0.0 || max_position_size_pct > 1.0 {
            return Err(RiskError::InvalidParameter(
                "max_position_size_pct should be between 0 and 1. 0 (100%)".to_string(),
            ));
        }

        if max_leverage < 1.0 || max_leverage > 100.0 {
            return Err(RiskError::InvalidParameter(
                "max_leverage should be between 1. 0 and 100.0".to_string(),
            ));
        }

        Ok(Self {
            max_risk_per_trade,
            max_total_risk,
            max_position_size_pct,
            max_leverage,
            active_risks: HashMap::new(),
        })
    }

    /// 创建保守型风险管理器（2%单笔，6%总风险，20%单仓，无杠杆）
    pub fn conservative() -> Self {
        Self::new(0.02, 0.06, 0.2, 1.0).unwrap()
    }

    /// 创建激进型风险管理器（5%单笔，15%总风险，30%单仓，2倍杠杆）
    pub fn aggressive() -> Self {
        Self::new(0.05, 0.15, 0.3, 2.0).unwrap()
    }

    /// 创建平衡型风险管理器（3%单笔，10%总风险，25%单仓，1.5倍杠杆）
    pub fn balanced() -> Self {
        Self::new(0.03, 0.10, 0.25, 1.5).unwrap()
    }

    /// 计算仓位大小（基于固定风险百分比）
    ///
    /// # 核心公式
    /// ```text
    /// 仓位数量 = (总资金 × 风险%) / |入场价 - 止损价|
    /// ```
    ///
    /// # 参数
    /// * `entry_price` - 计划入场价格
    /// * `stop_loss` - 止损价格
    /// * `total_capital` - 总可用资金
    /// * `symbol` - 交易品种（用于检查风险限制）
    ///
    /// # 返回
    /// * `Ok(PositionSize)` - 计算成功
    /// * `Err(RiskError)` - 超出风险限制或参数无效
    pub fn calculate_position_size(
        &self,
        entry_price: f64,
        stop_loss: f64,
        total_capital: f64,
        symbol: &Symbol,
    ) -> Result<PositionSize, RiskError> {
        // 参数验证
        if entry_price <= 0.0 || stop_loss <= 0.0 {
            return Err(RiskError::InvalidParameter(
                "Prices must be positive".to_string(),
            ));
        }

        if total_capital <= 0.0 {
            return Err(RiskError::InvalidParameter(
                "Total capital must be positive".to_string(),
            ));
        }

        // 计算价格差距（风险距离）
        let price_diff = (entry_price - stop_loss).abs();
        if price_diff == 0.0 {
            return Err(RiskError::InvalidParameter(
                "Entry price and stop loss cannot be the same".to_string(),
            ));
        }

        // 检查总风险敞口
        let current_total_risk = self.get_total_risk();
        let available_risk_pct = self.max_total_risk - current_total_risk;

        if available_risk_pct <= 0.0 {
            return Err(RiskError::ExceedsMaxTotalRisk {
                current: current_total_risk,
                max: self.max_total_risk,
            });
        }

        // 使用较小的风险百分比（单笔风险 vs 剩余可用风险）
        let effective_risk_pct = self.max_risk_per_trade.min(available_risk_pct);

        // 计算风险金额
        let risk_amount = total_capital * effective_risk_pct;

        // 计算基础仓位大小
        let mut size = risk_amount / price_diff;

        // 应用杠杆
        size *= self.max_leverage;

        // 计算持仓价值
        let position_value = size * entry_price;

        // 检查单仓占比限制
        let position_pct = position_value / total_capital;
        if position_pct > self.max_position_size_pct {
            // 按比例缩减仓位
            let scale_factor = self.max_position_size_pct / position_pct;
            size *= scale_factor;
        }

        Ok(PositionSize {
            size,
            entry_price,
            stop_loss,
            risk_amount,
            risk_pct: effective_risk_pct,
            position_value: size * entry_price,
        })
    }

    /// 计算止损价格（基于固定风险金额）
    ///
    /// # 公式
    /// ```text
    /// 止损价 = 入场价 ± (风险金额 / 仓位数量)
    /// ```
    pub fn calculate_stop_loss(
        &self,
        entry_price: f64,
        position_size: f64,
        total_capital: f64,
        is_long: bool,
    ) -> f64 {
        let risk_amount = total_capital * self.max_risk_per_trade;
        let price_diff = risk_amount / position_size;

        if is_long {
            entry_price - price_diff
        } else {
            entry_price + price_diff
        }
    }

    /// 计算止盈价格（基于风险回报比）
    ///
    /// # 参数
    /// * `entry_price` - 入场价
    /// * `stop_loss` - 止损价
    /// * `reward_risk_ratio` - 盈亏比（例如 2.0 表示 2:1）
    /// * `is_long` - 是否做多
    pub fn calculate_take_profit(
        &self,
        entry_price: f64,
        stop_loss: f64,
        reward_risk_ratio: f64,
        is_long: bool,
    ) -> f64 {
        let risk_distance = (entry_price - stop_loss).abs();
        let reward_distance = risk_distance * reward_risk_ratio;

        if is_long {
            entry_price + reward_distance
        } else {
            entry_price - reward_distance
        }
    }

    /// 注册新的风险敞口（开仓时调用）
    pub fn register_risk(&mut self, symbol: Symbol, risk_pct: f64) {
        *self.active_risks.entry(symbol).or_insert(0.0) += risk_pct;
    }

    /// 释放风险敞口（平仓时调用）
    pub fn release_risk(&mut self, symbol: &Symbol, risk_pct: f64) {
        if let Some(risk) = self.active_risks.get_mut(symbol) {
            *risk -= risk_pct;
            if *risk <= 0.0 {
                self.active_risks.remove(symbol);
            }
        }
    }

    /// 获取当前总风险敞口
    pub fn get_total_risk(&self) -> f64 {
        self.active_risks.values().sum()
    }

    /// 获取指定品种的风险敞口
    pub fn get_symbol_risk(&self, symbol: &Symbol) -> f64 {
        self.active_risks.get(symbol).copied().unwrap_or(0.0)
    }

    /// 检查是否可以开新仓
    pub fn can_open_position(&self, symbol: &Symbol) -> Result<(), RiskError> {
        let total_risk = self.get_total_risk();

        if total_risk >= self.max_total_risk {
            return Err(RiskError::ExceedsMaxTotalRisk {
                current: total_risk,
                max: self.max_total_risk,
            });
        }

        Ok(())
    }

    /// 计算凯利公式建议仓位（基于历史胜率和盈亏比）
    ///
    /// # 凯利公式
    /// ```text
    /// f* = (p × b - q) / b
    /// 其中：
    /// f* = 最优仓位比例
    /// p = 胜率
    /// q = 败率 (1 - p)
    /// b = 盈亏比 (平均盈利 / 平均亏损)
    /// ```
    pub fn kelly_criterion(win_rate: f64, avg_win: f64, avg_loss: f64) -> Result<f64, RiskError> {
        if win_rate <= 0.0 || win_rate >= 1.0 {
            return Err(RiskError::InvalidParameter(
                "Win rate must be between 0 and 1".to_string(),
            ));
        }

        if avg_win <= 0.0 || avg_loss <= 0.0 {
            return Err(RiskError::InvalidParameter(
                "Average win/loss must be positive".to_string(),
            ));
        }

        let lose_rate = 1.0 - win_rate;
        let reward_risk_ratio = avg_win / avg_loss;

        let kelly = (win_rate * reward_risk_ratio - lose_rate) / reward_risk_ratio;

        // 凯利公式往往过于激进，通常使用半凯利或1/4凯利
        let conservative_kelly = (kelly * 0.5).clamp(0.0, 0.1); // 最多10%

        Ok(conservative_kelly)
    }
}

/// 风险管理错误
#[derive(Debug, thiserror::Error)]
pub enum RiskError {
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("Exceeds maximum risk per trade: {current}% > {max}%")]
    ExceedsMaxRiskPerTrade { current: f64, max: f64 },

    #[error("Exceeds maximum total risk: {current}% > {max}%")]
    ExceedsMaxTotalRisk { current: f64, max: f64 },

    #[error("Exceeds maximum position size")]
    ExceedsMaxPositionSize,
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     #[test]
//     fn test_risk_manager_creation() {
//         let rm = RiskManager::conservative();
//         assert_eq!(rm.max_risk_per_trade, 0.02);
//         assert_eq!(rm.max_total_risk, 0.06);
//
//         let rm_agg = RiskManager::aggressive();
//         assert_eq!(rm_agg.max_risk_per_trade, 0.05);
//     }
//
//     #[test]
//     fn test_position_size_calculation() {
//         let rm = RiskManager::conservative();
//         let symbol = "BTC-USDT".into();
//
//         // 场景：10万资金，入场价50000，止损49000，价差1000
//         // 风险金额 = 100000 × 0.02 = 2000
//         // 仓位 = 2000 / 1000 = 2
//         let result = rm
//             .calculate_position_size(50000.0, 49000.0, 100_000.0, &symbol)
//             .unwrap();
//
//         assert_eq!(result.size, 2.0);
//         assert_eq!(result.risk_amount, 2000.0);
//         assert_eq!(result.risk_pct, 0.02);
//         assert_eq!(result.position_value, 100_000.0); // 2 × 50000
//     }
//
//     #[test]
//     fn test_max_position_size_limit() {
//         let rm = RiskManager::conservative(); // max_position_size_pct = 0.2 (20%)
//         let symbol = "BTC-USDT".into();
//
//         // 场景：10万资金，入场价100，止损99，价差1
//         // 风险金额 = 100000 × 0.02 = 2000
//         // 基础仓位 = 2000 / 1 = 2000
//         // 持仓价值 = 2000 × 100 = 200,000（超过20%限制）
//         // 应该被缩减到20%：100000 × 0.2 = 20000
//         // 调整后仓位 = 20000 / 100 = 200
//         let result = rm
//             .calculate_position_size(100.0, 99.0, 100_000.0, &symbol)
//             .unwrap();
//
//         assert_eq!(result.size, 200.0);
//         assert_eq!(result.position_value, 20_000.0);
//     }
//
//     #[test]
//     fn test_stop_loss_calculation() {
//         let rm = RiskManager::conservative();
//
//         // 做多：入场50000，仓位2，总资金100000
//         // 风险金额 = 100000 × 0.02 = 2000
//         // 止损距离 = 2000 / 2 = 1000
//         // 止损价 = 50000 - 1000 = 49000
//         let sl_long = rm.calculate_stop_loss(50000.0, 2.0, 100_000.0, true);
//         assert_eq!(sl_long, 49000.0);
//
//         // 做空：入场50000，止损应该在上方
//         let sl_short = rm.calculate_stop_loss(50000.0, 2.0, 100_000.0, false);
//         assert_eq!(sl_short, 51000.0);
//     }
//
//     #[test]
//     fn test_take_profit_calculation() {
//         let rm = RiskManager::conservative();
//
//         // 做多：入场100，止损90，盈亏比2:1
//         // 风险距离 = 10
//         // 盈利距离 = 10 × 2 = 20
//         // 止盈价 = 100 + 20 = 120
//         let tp_long = rm.calculate_take_profit(100.0, 90.0, 2.0, true);
//         assert_eq!(tp_long, 120.0);
//
//         // 做空：入场100，止损110，盈亏比2:1
//         // 止盈价 = 100 - 20 = 80
//         let tp_short = rm.calculate_take_profit(100.0, 110.0, 2.0, false);
//         assert_eq!(tp_short, 80.0);
//     }
//
//     #[test]
//     fn test_risk_tracking() {
//         let mut rm = RiskManager::conservative(); // max_total_risk = 0.06 (6%)
//
//         let btc: Symbol = "BTC-USDT".into();
//         let eth: Symbol = "ETH-USDT".into();
//
//         // 注册风险
//         rm.register_risk(btc.clone(), 0.02);
//         assert_eq!(rm.get_total_risk(), 0.02);
//
//         rm.register_risk(eth.clone(), 0.02);
//         assert_eq!(rm.get_total_risk(), 0.04);
//
//         // 再次开仓
//         rm.register_risk(btc.clone(), 0.02);
//         assert_eq!(rm.get_symbol_risk(&btc), 0.04);
//         assert_eq!(rm.get_total_risk(), 0.06);
//
//         // 应该达到上限
//         assert!(rm.can_open_position(&eth).is_err());
//
//         // 释放风险
//         rm.release_risk(&btc, 0.02);
//         assert_eq!(rm.get_total_risk(), 0.04);
//         assert!(rm.can_open_position(&eth).is_ok());
//     }
//
//     #[test]
//     fn test_kelly_criterion() {
//         // 场景：胜率60%，平均盈利15，平均亏损10
//         // 盈亏比 = 15/10 = 1.5
//         // 凯利 = (0.6 × 1.5 - 0.4) / 1.5 = 0.333...
//         // 半凯利 = 0. 167
//         let kelly = RiskManager::kelly_criterion(0.6, 15.0, 10.0).unwrap();
//         assert!((kelly - 0.167).abs() < 0.01);
//     }
//
//     #[test]
//     fn test_total_risk_limit() {
//         let rm = RiskManager::conservative(); // max_total_risk = 6%
//         let symbol = "BTC-USDT".into();
//
//         // 模拟已经有4%的风险敞口
//         let mut rm_with_risk = rm.clone();
//         rm_with_risk.register_risk("ETH-USDT".into(), 0.04);
//
//         // 现在只剩2%可用风险
//         // 即使max_risk_per_trade是2%，但总风险接近上限
//         let result = rm_with_risk
//             .calculate_position_size(100.0, 99.0, 100_000.0, &symbol)
//             .unwrap();
//
//         // 应该使用剩余的2%，而不是完整的2%
//         assert_eq!(result.risk_pct, 0.02);
//     }
// }
