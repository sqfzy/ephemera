use crate::{
    indicators::{Indicator, MA},
    strategies::Strategy,
};
use ephemera_shared::{CandleData, Signal, Symbol};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MACrossError {
    #[error("Insufficient data for MA calculation")]
    InsufficientData,
}

/// 双均线交叉策略
/// 当快线上穿慢线时买入，下穿时卖出
#[derive(Debug, Clone)]
pub struct MACrossStrategy {
    symbol: Symbol,
    fast_ma: MA,
    slow_ma: MA,
    prev_fast: Option<Decimal>,
    prev_slow: Option<Decimal>,
    position_size: Decimal,
}

impl MACrossStrategy {
    pub fn new(
        symbol: Symbol,
        fast_period: usize,
        slow_period: usize,
        position_size: Decimal,
    ) -> Self {
        Self {
            symbol,
            fast_ma: MA::new(fast_period),
            slow_ma: MA::new(slow_period),
            prev_fast: None,
            prev_slow: None,
            position_size,
        }
    }
}

impl Strategy for MACrossStrategy {
    type Input = CandleData;
    type Signal = Signal;
    type Error = MACrossError;

    async fn on_data(&mut self, candle: Self::Input) -> Result<Option<Self::Signal>, Self::Error> {
        if candle.symbol != self.symbol {
            return Ok(None);
        }

        let fast = self.fast_ma.update(candle.close);
        let slow = self.slow_ma.update(candle.close);

        let signal = match (fast, slow, self.prev_fast, self.prev_slow) {
            (Some(f), Some(s), Some(pf), Some(ps)) => {
                // 金叉：快线上穿慢线
                if pf <= ps && f > s {
                    Some(Signal::buy(
                        self.symbol.clone(),
                        candle.close,
                        self.position_size,
                        format!("MA金叉: 快线={}, 慢线={}", f, s),
                    ))
                }
                // 死叉：快线下穿慢线
                else if pf >= ps && f < s {
                    Some(Signal::sell(
                        self.symbol.clone(),
                        candle.close,
                        self.position_size,
                        format!("MA死叉: 快线={}, 慢线={}", f, s),
                    ))
                } else {
                    Some(Signal::Hold)
                }
            }
            _ => None,
        };

        self.prev_fast = fast;
        self.prev_slow = slow;

        Ok(signal)
    }

    fn name(&self) -> &str {
        "MA Cross Strategy"
    }

    fn reset(&mut self) {
        self.fast_ma.reset();
        self.slow_ma.reset();
        self.prev_fast = None;
        self.prev_slow = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ephemera_shared::CANDLE_INTERVAL_1M;
    use rust_decimal::dec;

    #[tokio::test]
    async fn test_ma_cross_strategy() {
        let symbol = "BTC-USDT";
        let mut strategy = MACrossStrategy::new(symbol.into(), 5, 10, dec!(1.0));

        // 模拟上升趋势
        for i in 1..=20 {
            let candle = CandleData {
                symbol: symbol.into(),
                interval_sc: CANDLE_INTERVAL_1M,
                open_timestamp_ms: i * 60000,
                open: dec!(100) + Decimal::from(i),
                high: dec!(101) + Decimal::from(i),
                low: dec!(99) + Decimal::from(i),
                close: dec!(100) + Decimal::from(i),
                volume: dec!(1000),
            };

            let result = strategy.on_data(candle).await.unwrap();
            if i >= 10 {
                assert!(result.is_some());
            }
        }
    }
}
