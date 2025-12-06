use crate::indicators::MACD;
use crate::{indicators::Indicator, strategies::Strategy};
use ephemera_shared::{CandleData, Signal, Symbol};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MACDStrategyError {
    #[error("Insufficient data for MACD calculation")]
    InsufficientData,
}

/// MACD策略
/// MACD线上穿信号线时买入
/// MACD线下穿信号线时卖出
#[derive(Debug, Clone)]
pub struct MACDStrategy {
    symbol: Symbol,
    macd: MACD,
    prev_histogram: Option<f64>,
    position_size: f64,
}

impl MACDStrategy {
    pub fn new(symbol: Symbol, position_size: f64) -> Self {
        Self {
            symbol,
            macd: MACD::default(),
            prev_histogram: None,
            position_size,
        }
    }
}

impl Strategy for MACDStrategy {
    type Input = CandleData;
    type Signal = Signal;
    type Error = MACDStrategyError;

    async fn on_data(&mut self, candle: Self::Input) -> Result<Option<Self::Signal>, Self::Error> {
        if candle.symbol != self.symbol {
            return Ok(None);
        }

        let macd_value = match self.macd.update(candle.close) {
            Some(v) => v,
            None => return Ok(None),
        };

        let signal = match self.prev_histogram {
            Some(prev_hist) => {
                // 柱状图从负转正：MACD上穿信号线
                if prev_hist <= 0.0 && macd_value.histogram > 0.0 {
                    Signal::buy(
                        self.symbol.clone(),
                        candle.close,
                        self.position_size,
                    )
                }
                // 柱状图从正转负：MACD下穿信号线
                else if prev_hist >= 0.0 && macd_value.histogram < 0.0 {
                    Signal::sell(
                        self.symbol.clone(),
                        candle.close,
                        self.position_size,
                    )
                } else {
                    Signal::Hold
                }
            }
            None => Signal::Hold,
        };

        self.prev_histogram = Some(macd_value.histogram);

        Ok(Some(signal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ephemera_shared::CANDLE_INTERVAL_MIN1;

    #[tokio::test]
    async fn test_macd_strategy() {
        let symbol = "BTC-USDT";
        let mut strategy = MACDStrategy::new(symbol.into(), 1.0);

        for i in 1..=50 {
            let candle = CandleData {
                symbol: symbol.into(),
                interval_sc: CANDLE_INTERVAL_MIN1,
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

