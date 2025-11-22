use crate::indicators::RSI;
use crate::{indicators::Indicator, strategies::Strategy};
use ephemera_shared::{CandleData, Signal, Symbol};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RSIStrategyError {
    #[error("Insufficient data for RSI calculation")]
    InsufficientData,
}

/// RSI策略
/// RSI < 30 超卖，买入
/// RSI > 70 超买，卖出
#[derive(Debug, Clone)]
pub struct RSIStrategy {
    symbol: Symbol,
    rsi: RSI,
    oversold_threshold: f64,
    overbought_threshold: f64,
    position_size: f64,
}

impl RSIStrategy {
    pub fn new(
        symbol: Symbol,
        period: usize,
        oversold: f64,
        overbought: f64,
        position_size: f64,
    ) -> Self {
        Self {
            symbol,
            rsi: RSI::new(period),
            oversold_threshold: oversold,
            overbought_threshold: overbought,
            position_size,
        }
    }

    /// 默认参数：周期14，超卖30，超买70
    pub fn default_with_symbol(symbol: Symbol, position_size: f64) -> Self {
        Self::new(symbol, 14, 30.0, 70.0, position_size)
    }
}

impl Strategy for RSIStrategy {
    type Input = CandleData;
    type Signal = Signal;
    type Error = RSIStrategyError;

    async fn on_data(&mut self, candle: Self::Input) -> Result<Option<Self::Signal>, Self::Error> {
        if candle.symbol != self.symbol {
            return Ok(None);
        }

        let rsi_value = match self.rsi.update(candle.close) {
            Some(v) => v,
            None => return Ok(None),
        };

        let signal = if rsi_value < self.oversold_threshold {
            Signal::buy(
                self.symbol.clone(),
                candle.close,
                self.position_size,
                format!("RSI超卖: {}", rsi_value),
            )
        } else if rsi_value > self.overbought_threshold {
            Signal::sell(
                self.symbol.clone(),
                candle.close,
                self.position_size,
                format!("RSI超买: {}", rsi_value),
            )
        } else {
            Signal::Hold
        };

        Ok(Some(signal))
    }

    fn name(&self) -> &str {
        "RSI Strategy"
    }

    fn reset(&mut self) {
        self.rsi.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ephemera_shared::CANDLE_INTERVAL_1M;

    #[tokio::test]
    async fn test_rsi_strategy() {
        let symbol = "BTC-USDT";
        let mut strategy = RSIStrategy::default_with_symbol(symbol.into(), 1.0);

        for i in 1..=20 {
            let candle = CandleData {
                symbol: symbol.into(),
                interval_sc: CANDLE_INTERVAL_1M,
                open_timestamp_ms: i * 60000,
                open: 100.0 + i as f64,
                high: 101.0 + i as f64,
                low: 99.0 + i as f64,
                close: 100.0 + i as f64,
                volume: 1000.0,
            };

            let _ = strategy.on_data(candle).await;
        }
    }
}
