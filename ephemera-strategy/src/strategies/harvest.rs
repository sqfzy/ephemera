use crate::indicators::{Indicator, MVRVZScore, PiCycleTop};
use crate::strategies::Strategy;
use chrono::{DateTime, Utc};
use ephemera_shared::{CandleData, Signal, Symbol};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HarvestError {
    #[error("Insufficient data for exit analysis")]
    InsufficientData,
    #[error("Invalid exit configuration:  {0}")]
    InvalidConfig(String),
}

/// 收获/止盈策略
///
/// 多层级退出机制：
/// 1. Pi Cycle Top 信号：清仓 70%
/// 2. MVRV Z-Score > 7. 0：卖出 50%
/// 3. MVRV Z-Score > 6.0：卖出 30%
/// 4. MVRV Z-Score > 5.0：卖出 15%
/// 5. 时间熔断：2025-12-31 强制减仓至 50%
#[derive(Debug, Clone)]
pub struct HarvestStrategy {
    symbol: Symbol,

    /// 指标
    pi_cycle: PiCycleTop,
    mvrv: MVRVZScore,

    /// 配置
    config: HarvestConfig,

    /// 状态
    state: HarvestState,
}

#[derive(Debug, Clone)]
pub struct HarvestConfig {
    /// MVRV 卖出阈值
    pub mvrv_extreme_threshold: f64, // > 7.0
    pub mvrv_high_threshold: f64,   // > 6.0
    pub mvrv_medium_threshold: f64, // > 5.0

    /// 对应的卖出比例
    pub extreme_sell_pct: f64, // 0.5 (50%)
    pub high_sell_pct: f64,   // 0.3 (30%)
    pub medium_sell_pct: f64, // 0.15 (15%)

    /// Pi Cycle 触发后的卖出比例
    pub pi_cycle_sell_pct: f64, // 0.7 (70%)

    /// 时间熔断日期
    pub time_fuse_date: DateTime<Utc>,
    /// 时间熔断卖出比例
    pub time_fuse_sell_pct: f64, // 0.5 (50%)

    /// 是否启用分批卖出（Dollar-Cost Selling）
    pub enable_gradual_sell: bool,
    /// 分批卖出天数
    pub gradual_sell_days: u32,
}

impl Default for HarvestConfig {
    fn default() -> Self {
        Self {
            mvrv_extreme_threshold: 7.0,
            mvrv_high_threshold: 6.0,
            mvrv_medium_threshold: 5.0,

            extreme_sell_pct: 0.5,
            high_sell_pct: 0.3,
            medium_sell_pct: 0.15,

            pi_cycle_sell_pct: 0.7,

            // 默认 2025-12-31
            time_fuse_date: DateTime::from_timestamp(1735689600, 0).unwrap(),
            time_fuse_sell_pct: 0.5,

            enable_gradual_sell: true,
            gradual_sell_days: 7,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct HarvestState {
    /// 已触发的退出级别
    triggered_levels: Vec<ExitLevel>,
    /// 累计卖出比例
    total_sold_percentage: f64,
    /// 最后卖出时间
    last_sell_time: Option<DateTime<Utc>>,
    /// 是否已触发时间熔断
    time_fuse_triggered: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ExitLevel {
    PiCycleTop,
    MVRVExtreme,
    MVRVHigh,
    MVRVMedium,
    TimeFuse,
}

impl HarvestStrategy {
    pub fn new(symbol: Symbol, config: HarvestConfig) -> Self {
        Self {
            symbol,
            pi_cycle: PiCycleTop::new(),
            mvrv: MVRVZScore::new(365),
            config,
            state: HarvestState::default(),
        }
    }

    pub fn new_with_defaults(symbol: Symbol) -> Self {
        Self::new(symbol, HarvestConfig::default())
    }

    /// 检查 Pi Cycle Top 信号
    fn check_pi_cycle(&mut self, price: f64) -> Option<f64> {
        if let Some(pi_value) = self.pi_cycle.next(price) {
            if pi_value.is_top_signal
                && !self.state.triggered_levels.contains(&ExitLevel::PiCycleTop)
            {
                self.state.triggered_levels.push(ExitLevel::PiCycleTop);
                tracing::warn!(
                    symbol = %self.symbol,
                    "Pi Cycle Top signal triggered!"
                );
                return Some(self.config.pi_cycle_sell_pct);
            }
        }
        None
    }

    /// 检查 MVRV Z-Score 信号
    fn check_mvrv(&mut self, z_score: f64) -> Option<f64> {
        if z_score > self.config.mvrv_extreme_threshold
            && !self
                .state
                .triggered_levels
                .contains(&ExitLevel::MVRVExtreme)
        {
            self.state.triggered_levels.push(ExitLevel::MVRVExtreme);
            tracing::warn!(
                symbol = %self.symbol,
                z_score = z_score,
                "MVRV extreme overvaluation detected!"
            );
            return Some(self.config.extreme_sell_pct);
        } else if z_score > self.config.mvrv_high_threshold
            && !self.state.triggered_levels.contains(&ExitLevel::MVRVHigh)
        {
            self.state.triggered_levels.push(ExitLevel::MVRVHigh);
            tracing::warn!(
                symbol = %self. symbol,
                z_score = z_score,
                "MVRV high overvaluation detected!"
            );
            return Some(self.config.high_sell_pct);
        } else if z_score > self.config.mvrv_medium_threshold
            && !self.state.triggered_levels.contains(&ExitLevel::MVRVMedium)
        {
            self.state.triggered_levels.push(ExitLevel::MVRVMedium);
            tracing:: info!(
                symbol = %self. symbol,
                z_score = z_score,
                "MVRV medium overvaluation detected"
            );
            return Some(self.config.medium_sell_pct);
        }

        None
    }

    /// 检查时间熔断
    fn check_time_fuse(&mut self, current_time: DateTime<Utc>) -> Option<f64> {
        if current_time >= self.config.time_fuse_date && !self.state.time_fuse_triggered {
            self.state.time_fuse_triggered = true;
            self.state.triggered_levels.push(ExitLevel::TimeFuse);
            tracing::error!(
                symbol = %self. symbol,
                "Time fuse triggered!  Force selling {:.0}%",
                self.config.time_fuse_sell_pct * 100.0
            );
            return Some(self.config.time_fuse_sell_pct);
        }
        None
    }

    /// 生成卖出信号
    fn generate_sell_signal(
        &mut self,
        sell_percentage: f64,
        current_price: f64,
        current_time: DateTime<Utc>,
    ) -> Signal {
        self.state.total_sold_percentage += sell_percentage;
        self.state.last_sell_time = Some(current_time);

        // 这里的 size 需要从外部获取当前持仓
        // 为了示例，我们返回比例，实际使用时需要配合持仓管理
        Signal::sell(self.symbol.clone(), current_price, sell_percentage)
    }

    /// 获取累计卖出比例
    pub fn get_total_sold_percentage(&self) -> f64 {
        self.state.total_sold_percentage
    }

    /// 重置策略（用于新周期）
    pub fn reset(&mut self) {
        self.state = HarvestState::default();
        self.pi_cycle.reset_signal();
    }
}

impl Strategy for HarvestStrategy {
    type Input = (CandleData, Option<f64>); // (candle, optional_mvrv_z_score)
    type Signal = Signal;
    type Error = HarvestError;

    async fn on_data(&mut self, input: Self::Input) -> Result<Option<Self::Signal>, Self::Error> {
        let (candle, mvrv_z_score_opt) = input;

        if candle.symbol != self.symbol {
            return Ok(None);
        }

        let current_time = DateTime::from_timestamp_millis(candle.open_timestamp_ms as i64)
            .unwrap_or_else(Utc::now);

        // 优先级 1: 检查 Pi Cycle Top
        if let Some(sell_pct) = self.check_pi_cycle(candle.close) {
            return Ok(Some(self.generate_sell_signal(
                sell_pct,
                candle.close,
                current_time,
            )));
        }

        // 优先级 2: 检查时间熔断
        if let Some(sell_pct) = self.check_time_fuse(current_time) {
            return Ok(Some(self.generate_sell_signal(
                sell_pct,
                candle.close,
                current_time,
            )));
        }

        // 优先级 3: 检查 MVRV Z-Score（如果有链上数据）
        if let Some(z_score) = mvrv_z_score_opt {
            if let Some(sell_pct) = self.check_mvrv(z_score) {
                return Ok(Some(self.generate_sell_signal(
                    sell_pct,
                    candle.close,
                    current_time,
                )));
            }
        }

        Ok(Some(Signal::Hold))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ephemera_shared::CANDLE_INTERVAL_MIN1;

    #[tokio::test]
    async fn test_harvest_strategy() {
        let mut strategy = HarvestStrategy::new_with_defaults("BTC-USDT".into());

        let now = Utc::now();

        // 模拟 Pi Cycle 触发
        for day in 1..=400 {
            let candle = CandleData {
                symbol: "BTC-USDT".into(),
                interval_sc: CANDLE_INTERVAL_MIN1,
                open_timestamp_ms: (now.timestamp_millis() + day * 86400000) as u64,
                open: 100000.0,
                high: 101000.0,
                low: 99000.0,
                close: 100000.0 + (day as f64 * 100.0),
                volume: 1000.0,
            };

            let result = strategy.on_data((candle, None)).await;

            if let Ok(Some(signal)) = result {
                if !signal.is_hold() {
                    println!("Day {}: Exit signal triggered: {:?}", day, signal);
                }
            }
        }
    }

    #[tokio::test]
    async fn test_mvrv_exit() {
        let mut strategy = HarvestStrategy::new_with_defaults("BTC-USDT".into());

        let candle = CandleData {
            symbol: "BTC-USDT".into(),
            interval_sc: CANDLE_INTERVAL_MIN1,
            open_timestamp_ms: Utc::now().timestamp_millis() as u64,
            open: 100000.0,
            high: 101000.0,
            low: 99000.0,
            close: 100000.0,
            volume: 1000.0,
        };

        // 测试 MVRV 极度高估
        let result = strategy.on_data((candle.clone(), Some(7.5))).await.unwrap();
        assert!(result.is_some());
        if let Some(Signal::Sell { size, .. }) = result {
            assert_eq!(size, 0.5);
        }
    }
}
