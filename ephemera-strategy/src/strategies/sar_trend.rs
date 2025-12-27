use crate::context::StrategyContext;
use crate::indicators::{EMA, Indicator, SAR, SARValue};
use crate::risk::RiskManager;
use crate::strategies::Strategy;
use ephemera_shared::{CandleData, Signal, Symbol};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SARTrendError {
    #[error("Insufficient data for calculation")]
    InsufficientData,
    #[error("Invalid parameters: {0}")]
    InvalidParameters(String),
}

/// SAR 趋势捕手策略
///
/// # 核心逻辑
///
/// **第一步：趋势过滤（筛选器）**
/// - 只有当价格位于 EMA 200 上方时，才考虑做多
/// - 只有当价格位于 EMA 200 下方时，才考虑做空
///
/// **第二步：进场信号（触发器）**
/// - 做多：SAR 从 K 线上方翻转到下方
/// - 做空：SAR 从 K 线下方翻转到上方
///
/// **第三步：初始止损**
/// - 做多：最近的波段低点
/// - 做空：最近的波段高点
///
/// **第四步：止盈与出场（移动止损）**
/// - 价格跌破 SAR 点位时无条件平仓
/// - SAR 每天自动向有利方向移动
#[derive(Debug, Clone)]
pub struct SARTrendStrategy {
    symbol: Symbol,

    // 指标
    ema200: EMA,
    sar: SAR,

    // 前一根 K 线的 SAR 值（用于检测翻转）
    prev_sar_value: Option<SARValue>,

    risk_manager: RiskManager,

    // 用于记录波段低点/高点（计算初始止损）
    recent_swing_low: Option<f64>,
    recent_swing_high: Option<f64>,
    swing_lookback: usize,          // 波段回溯周期
    price_history: Vec<(f64, f64)>, // (high, low) 历史
}

impl SARTrendStrategy {
    pub fn new(symbol: Symbol, risk_manager: RiskManager, swing_lookback: usize) -> Self {
        Self {
            symbol,
            ema200: EMA::new(200),
            sar: SAR::default(),
            prev_sar_value: None,
            risk_manager,
            recent_swing_low: None,
            recent_swing_high: None,
            swing_lookback,
            price_history: Vec::with_capacity(swing_lookback),
        }
    }

    /// 使用保守型风险管理创建策略
    pub fn default_with_symbol(symbol: Symbol) -> Self {
        Self::new(symbol, RiskManager::conservative(), 20)
    }

    /// 更新价格历史并计算波段高低点
    fn update_swing_points(&mut self, high: f64, low: f64) {
        self.price_history.push((high, low));

        // 保持固定长度
        if self.price_history.len() > self.swing_lookback {
            self.price_history.remove(0);
        }

        // 计算最近的波段低点（用于做多止损）
        if self.price_history.len() >= 3 {
            self.recent_swing_low = self.find_swing_low();
            self.recent_swing_high = self.find_swing_high();
        }
    }

    /// 寻找波段低点（简化版：最近 N 根 K 线的最低点）
    fn find_swing_low(&self) -> Option<f64> {
        self.price_history
            .iter()
            .map(|(_, low)| *low)
            .min_by(|a, b| a.partial_cmp(b).unwrap())
    }

    /// 寻找波段高点（简化版：最近 N 根 K 线的最高点）
    fn find_swing_high(&self) -> Option<f64> {
        self.price_history
            .iter()
            .map(|(high, _)| *high)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
    }

    /// 检查入场信号
    fn check_entry_signal(
        &self,
        close_price: f64,
        ema200: f64,
        current_sar: SARValue,
        total_capital: f64,
    ) -> Option<Signal> {
        let prev_sar = self.prev_sar_value?;

        // 检测 SAR 翻转
        let sar_flipped_to_uptrend = !prev_sar.is_uptrend && current_sar.is_uptrend;
        let sar_flipped_to_downtrend = prev_sar.is_uptrend && !current_sar.is_uptrend;

        // 做多条件：价格在 EMA 200 上方 && SAR 翻转向上
        if close_price > ema200 && sar_flipped_to_uptrend {
            // 使用波段低点作为初始止损，如果没有则使用当前 SAR 值
            let stop_loss = self.recent_swing_low.unwrap_or(current_sar.sar);

            if let Ok(position_size) = self.risk_manager.calculate_position_size(
                close_price,
                stop_loss,
                total_capital,
                &self.symbol,
            ) {
                return Some(Signal::Buy {
                    symbol: self.symbol.clone(),
                    price: close_price,
                    size: position_size.size,
                });
            }
        }

        // 做空条件：价格在 EMA 200 下方 && SAR 翻转向下
        if close_price < ema200 && sar_flipped_to_downtrend {
            let stop_loss = self.recent_swing_high.unwrap_or(current_sar.sar);

            if let Ok(position_size) = self.risk_manager.calculate_position_size(
                close_price,
                stop_loss,
                total_capital,
                &self.symbol,
            ) {
                return Some(Signal::Sell {
                    symbol: self.symbol.clone(),
                    price: close_price,
                    size: position_size.size,
                });
            }
        }

        None
    }

    /// 检查离场信号（移动止损）
    fn check_exit_signal(
        &self,
        close_price: f64,
        current_sar: SARValue,
        context: &StrategyContext,
    ) -> Option<Signal> {
        let position = context.get_position(&self.symbol)?;

        let is_long = position.size > 0.0;
        let is_short = position.size < 0.0;

        // 做多止损：价格跌破 SAR
        if is_long && close_price < current_sar.sar {
            return Some(Signal::Sell {
                symbol: self.symbol.clone(),
                price: close_price,
                size: position.size,
            });
        }

        // 做空止损：价格涨破 SAR
        if is_short && close_price > current_sar.sar {
            return Some(Signal::Buy {
                symbol: self.symbol.clone(),
                price: close_price,
                size: position.size.abs(),
            });
        }

        // 持有
        Some(Signal::Hold)
    }
}

impl Strategy for SARTrendStrategy {
    type Input = CandleData;
    type Signal = Signal;
    type Error = SARTrendError;

    async fn on_data(
        &mut self,
        candle: Self::Input,
        context: &StrategyContext,
    ) -> Result<Option<Self::Signal>, Self::Error> {
        if candle.symbol != self.symbol {
            return Ok(None);
        }

        // 更新 EMA 200
        let ema200_value = match self.ema200.update(candle.close) {
            Some(v) => v,
            None => return Ok(None), // 数据不足（需要 200 根 K 线）
        };

        // 更新 SAR
        let sar_value = match self.sar.update((candle.high, candle.low)) {
            Some(v) => v,
            None => return Ok(None),
        };

        // 更新波段点位
        self.update_swing_points(candle.high, candle.low);

        // 判断是否有持仓
        let has_position = context.get_position(&self.symbol).is_some();

        // 生成信号
        let signal = if has_position {
            // 有持仓：检查是否需要止损离场
            self.check_exit_signal(candle.close, sar_value, context)
        } else {
            // 无持仓：检查是否有入场信号
            self.check_entry_signal(candle.close, ema200_value, sar_value, context.total_balance)
        };

        // 保存当前 SAR 值用于下次比较
        self.prev_sar_value = Some(sar_value);

        Ok(signal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::StrategyContext;
    use ephemera_shared::CANDLE_INTERVAL_H4;

    #[tokio::test]
    async fn test_swing_point_detection() {
        let mut strategy = SARTrendStrategy::default_with_symbol("BTC-USDT".into());

        // 模拟价格数据
        let prices = vec![
            (100.0, 95.0),  // K1
            (102.0, 97.0),  // K2
            (101.0, 96.0),  // K3
            (105.0, 100.0), // K4
            (103.0, 98.0),  // K5 - 低点应该是 95. 0
        ];

        for (high, low) in prices {
            strategy.update_swing_points(high, low);
        }

        assert_eq!(strategy.recent_swing_low, Some(95.0));
        assert_eq!(strategy.recent_swing_high, Some(105.0));
    }

    #[tokio::test]
    async fn test_full_strategy_flow() {
        let mut strategy = SARTrendStrategy::default_with_symbol("BTC-USDT".into());
        let mut context = StrategyContext::new(100_000.0);

        // 需要至少 200 根 K 线来初始化 EMA 200
        // 这里只测试基本流程
        for i in 1..=210 {
            let candle = CandleData {
                symbol: "BTC-USDT".into(),
                interval_sc: CANDLE_INTERVAL_H4,
                open_timestamp_ms: i * 14400000, // 4小时 = 14400秒
                open: 50000.0 + i as f64 * 10.0,
                high: 50100.0 + i as f64 * 10.0,
                low: 49900.0 + i as f64 * 10.0,
                close: 50000.0 + i as f64 * 10.0,
                volume: 100.0,
            };

            match strategy.on_data(candle.clone(), &context).await {
                Ok(Some(Signal::Buy {
                    symbol,
                    price,
                    size,
                })) => {
                    println!("📈 [K{}] 买入信号: {} @ {} x {}", i, symbol, price, size);
                    context.add_position(symbol, size, price);
                    context.available_balance -= size * price;
                }
                Ok(Some(Signal::Sell {
                    symbol,
                    price,
                    size,
                })) => {
                    println!("📉 [K{}] 卖出信号: {} @ {} x {}", i, symbol, price, size);
                    context.reduce_position(&symbol, size);
                    context.available_balance += size * price;
                }
                Ok(Some(Signal::Hold)) => {
                    // 持有
                }
                Ok(None) => {
                    // 数据不足
                }
                Err(e) => {
                    eprintln!("错误: {}", e);
                }
            }
        }
    }
}
