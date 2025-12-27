use crate::indicators::{AHR, Indicator, RSI};
use crate::strategies::Strategy;
use chrono::{DateTime, Utc};
use ephemera_shared::{CandleData, Signal, Symbol};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SmartDCAError {
    #[error("Insufficient data for indicators")]
    InsufficientData,
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

/// 智能动态定投策略
///
/// 核心理念：
/// - 使用 AHR999 进行宏观估值判断
/// - 使用 RSI 进行微观择时优化
/// - 根据市场状态动态调整定投金额
///
/// 定投逻辑：
/// - AHR999 < 0.45 (抄底区): 2. 5x 基础金额
/// - AHR999 0.45-0.80 (低估区): 1.5x 基础金额
/// - AHR999 0.80-1.20 (正常区): 1.0x 基础金额
/// - AHR999 > 1.20 (高估区): 停止定投
/// - RSI < 30: 额外增加 20%
/// - RSI > 70: 减少 20%
#[derive(Debug, Clone)]
pub struct SmartDCAStrategy {
    /// 交易对
    symbol: Symbol,

    /// 每日基础定投金额 (USDT)
    base_daily_amount: f64,

    /// 指标
    ahr999: AHR,
    rsi: RSI,

    /// 配置参数
    config: DCAConfig,

    /// 状态追踪
    state: DCAState,
}

#[derive(Debug, Clone)]
pub struct DCAConfig {
    /// AHR999 抄底阈值
    pub ahr999_bottom_threshold: f64,
    /// AHR999 低估阈值
    pub ahr999_undervalued_threshold: f64,
    /// AHR999 正常阈值上限
    pub ahr999_fair_threshold: f64,

    /// RSI 超卖阈值
    pub rsi_oversold: f64,
    /// RSI 超买阈值
    pub rsi_overbought: f64,

    /// 抄底区乘数
    pub bottom_multiplier: f64,
    /// 低估区乘数
    pub undervalued_multiplier: f64,
    /// 正常区乘数
    pub fair_multiplier: f64,

    /// RSI 调整因子
    pub rsi_adjustment_factor: f64,

    /// 紧急资金储备比例 (用于黑天鹅事件)
    pub emergency_fund_ratio: f64,

    /// 最大单次买入乘数限制
    pub max_single_multiplier: f64,
}

impl Default for DCAConfig {
    fn default() -> Self {
        Self {
            ahr999_bottom_threshold: 0.45,
            ahr999_undervalued_threshold: 0.80,
            ahr999_fair_threshold: 1.20,

            rsi_oversold: 30.0,
            rsi_overbought: 70.0,

            bottom_multiplier: 2.5,
            undervalued_multiplier: 1.5,
            fair_multiplier: 1.0,

            rsi_adjustment_factor: 0.2,

            emergency_fund_ratio: 0.20,

            max_single_multiplier: 3.0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DCAState {
    /// 累计定投次数
    pub total_dca_count: u64,
    /// 累计投入金额 (USDT)
    pub total_invested: f64,
    /// 累计买入数量 (BTC/SOL/ETH)
    pub total_quantity: f64,
    /// 平均成本
    pub average_cost: f64,
    /// 最后定投时间
    pub last_dca_time: Option<DateTime<Utc>>,
}

impl SmartDCAStrategy {
    pub fn new(
        symbol: Symbol,
        base_daily_amount: f64,
        config: DCAConfig,
    ) -> Result<Self, SmartDCAError> {
        if base_daily_amount <= 0.0 {
            return Err(SmartDCAError::InvalidConfig(
                "base_daily_amount must be positive".to_string(),
            ));
        }

        Ok(Self {
            symbol,
            base_daily_amount,
            ahr999: AHR::new(),
            rsi: RSI::new(14),
            config,
            state: DCAState::default(),
        })
    }

    /// 使用默认配置创建策略
    pub fn new_with_defaults(
        symbol: Symbol,
        base_daily_amount: f64,
    ) -> Result<Self, SmartDCAError> {
        Self::new(symbol, base_daily_amount, DCAConfig::default())
    }

    /// 计算动态乘数
    fn calculate_multiplier(&self, ahr999_value: f64, rsi_value: f64) -> f64 {
        // 第一步：基于 AHR999 的宏观调整
        let mut multiplier = if ahr999_value < self.config.ahr999_bottom_threshold {
            self.config.bottom_multiplier
        } else if ahr999_value < self.config.ahr999_undervalued_threshold {
            self.config.undervalued_multiplier
        } else if ahr999_value < self.config.ahr999_fair_threshold {
            self.config.fair_multiplier
        } else {
            0.0 // 高估区停止定投
        };

        // 第二步：基于 RSI 的微观调整
        if multiplier > 0.0 {
            if rsi_value < self.config.rsi_oversold {
                // 超卖，增加投入
                multiplier *= 1.0 + self.config.rsi_adjustment_factor;
            } else if rsi_value > self.config.rsi_overbought {
                // 超买，减少投入
                multiplier *= 1.0 - self.config.rsi_adjustment_factor;
            }
        }

        // 限制最大乘数
        multiplier.min(self.config.max_single_multiplier)
    }

    /// 计算本次应买入的金额
    fn calculate_buy_amount(&self, multiplier: f64) -> f64 {
        self.base_daily_amount * multiplier
    }

    /// 更新状态
    fn update_state(&mut self, price: f64, quantity: f64, timestamp: DateTime<Utc>) {
        self.state.total_dca_count += 1;
        self.state.total_invested += price * quantity;
        self.state.total_quantity += quantity;
        self.state.average_cost = self.state.total_invested / self.state.total_quantity;
        self.state.last_dca_time = Some(timestamp);
    }

    /// 获取策略状态
    pub fn get_state(&self) -> &DCAState {
        &self.state
    }

    /// 获取当前持仓盈亏比例
    pub fn get_pnl_percentage(&self, current_price: f64) -> f64 {
        if self.state.average_cost == 0.0 {
            0.0
        } else {
            (current_price - self.state.average_cost) / self.state.average_cost * 100.0
        }
    }
}

impl Strategy for SmartDCAStrategy {
    type Input = CandleData;
    type Signal = Signal;
    type Error = SmartDCAError;

    async fn on_data(&mut self, candle: Self::Input) -> Result<Option<Self::Signal>, Self::Error> {
        // 检查交易对
        if candle.symbol != self.symbol {
            return Ok(None);
        }

        let timestamp = DateTime::from_timestamp_millis(candle.open_timestamp_ms as i64)
            .unwrap_or_else(Utc::now);

        // 更新 AHR999 指标
        let ahr999_value = match self.ahr999.next((candle.close, timestamp)) {
            Some(v) => v,
            None => {
                tracing::debug!("AHR999 indicator not ready yet");
                return Ok(None);
            }
        };

        // 更新 RSI 指标
        let rsi_value = match self.rsi.next(candle.close) {
            Some(v) => v,
            None => {
                tracing::debug!("RSI indicator not ready yet");
                return Ok(None);
            }
        };

        // 计算动态乘数
        let multiplier = self.calculate_multiplier(ahr999_value, rsi_value);

        // 记录指标日志
        tracing::info!(
            symbol = %self.symbol,
            price = candle.close,
            ahr999 = ahr999_value,
            rsi = rsi_value,
            multiplier = multiplier,
            "DCA indicators updated"
        );

        // 如果乘数为0，表示市场高估，停止定投
        if multiplier == 0.0 {
            tracing::warn!(
                symbol = %self. symbol,
                ahr999 = ahr999_value,
                "Market overvalued, DCA paused"
            );
            return Ok(Some(Signal::Hold));
        }

        // 计算买入金额和数量
        let buy_amount_usdt = self.calculate_buy_amount(multiplier);
        let buy_quantity = buy_amount_usdt / candle.close;

        // 更新状态
        self.update_state(candle.close, buy_quantity, timestamp);

        // 生成买入信号
        let signal = Signal::buy(self.symbol.clone(), candle.close, buy_quantity);

        tracing::info!(
            symbol = %self.symbol,
            price = candle.close,
            usdt_amount = buy_amount_usdt,
            quantity = buy_quantity,
            total_invested = self.state.total_invested,
            avg_cost = self.state.average_cost,
            pnl_pct = self.get_pnl_percentage(candle.close),
            "DCA buy signal generated"
        );

        Ok(Some(signal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ephemera_shared::CANDLE_INTERVAL_MIN1;

    #[tokio::test]
    async fn test_smart_dca_strategy() {
        let symbol = "BTC-USDT";
        let base_amount = 33.33; // ~$1000/month ÷ 30 days

        let mut strategy = SmartDCAStrategy::new_with_defaults(symbol.into(), base_amount).unwrap();

        let now = Utc::now();

        // 模拟200天的数据（激活 AHR999）
        for day in 1..=200 {
            let candle = CandleData {
                symbol: symbol.into(),
                interval_sc: CANDLE_INTERVAL_MIN1,
                open_timestamp_ms: (now.timestamp_millis() + day * 86400000) as u64,
                open: 50000.0,
                high: 51000.0,
                low: 49000.0,
                close: 50000.0 + (day as f64 * 10.0),
                volume: 1000.0,
            };

            let result = strategy.on_data(candle.clone()).await;

            if day > 14 && result.is_ok() {
                let signal = result.unwrap();
                if day % 50 == 0 {
                    println!("Day {}: {:?}", day, signal);
                    println!("State: {:?}", strategy.get_state());
                }
            }
        }

        // 验证状态
        let state = strategy.get_state();
        assert!(state.total_dca_count > 0);
        assert!(state.total_invested > 0.0);
        assert!(state.average_cost > 0.0);
    }

    #[test]
    fn test_multiplier_calculation() {
        let strategy = SmartDCAStrategy::new_with_defaults("BTC-USDT".into(), 100.0).unwrap();

        // 测试抄底区
        let multiplier = strategy.calculate_multiplier(0.3, 50.0);
        assert_eq!(multiplier, 2.5);

        // 测试低估区
        let multiplier = strategy.calculate_multiplier(0.6, 50.0);
        assert_eq!(multiplier, 1.5);

        // 测试正常区
        let multiplier = strategy.calculate_multiplier(1.0, 50.0);
        assert_eq!(multiplier, 1.0);

        // 测试高估区
        let multiplier = strategy.calculate_multiplier(1.5, 50.0);
        assert_eq!(multiplier, 0.0);

        // 测试 RSI 超卖调整
        let multiplier = strategy.calculate_multiplier(1.0, 25.0);
        assert_eq!(multiplier, 1.2); // 1.0 * (1 + 0.2)

        // 测试 RSI 超买调整
        let multiplier = strategy.calculate_multiplier(1.0, 75.0);
        assert_eq!(multiplier, 0.8); // 1.0 * (1 - 0.2)
    }

    #[test]
    fn test_pnl_calculation() {
        let mut strategy = SmartDCAStrategy::new_with_defaults("BTC-USDT".into(), 100.0).unwrap();

        // 模拟买入
        strategy.state.total_invested = 10000.0;
        strategy.state.total_quantity = 0.2;
        strategy.state.average_cost = 50000.0;

        // 当前价格上涨 20%
        let pnl = strategy.get_pnl_percentage(60000.0);
        assert_eq!(pnl, 20.0);

        // 当前价格下跌 10%
        let pnl = strategy.get_pnl_percentage(45000.0);
        assert_eq!(pnl, -10.0);
    }
}
