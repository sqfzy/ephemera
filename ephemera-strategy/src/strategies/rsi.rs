use crate::indicators::RSI;
use crate::{indicators::Indicator, strategies::Strategy};
use ephemera_shared::{CandleData, Signal, Symbol};
use rust_decimal::Decimal;
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
    oversold_threshold: Decimal,
    overbought_threshold: Decimal,
    position_size: Decimal,
}

impl RSIStrategy {
    pub fn new(
        symbol: Symbol,
        period: usize,
        oversold: Decimal,
        overbought: Decimal,
        position_size: Decimal,
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
    pub fn default_with_symbol(symbol: Symbol, position_size: Decimal) -> Self {
        Self::new(symbol, 14, dec!(30), dec!(70), position_size)
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

use rust_decimal::dec;

#[cfg(test)]
mod tests {
    use super::*;
    use ephemera_shared::CANDLE_INTERVAL_1M;

    #[tokio::test]
    async fn test_rsi_strategy() {
        let symbol = "BTC-USDT";
        let mut strategy = RSIStrategy::default_with_symbol(symbol.into(), dec!(1.0));

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

            let _ = strategy.on_data(candle).await;
        }
    }
}
