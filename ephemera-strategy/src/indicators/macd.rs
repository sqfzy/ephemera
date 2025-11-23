use super::Indicator;
use crate::indicators::EMA;

/// MACD指标
#[derive(Debug, Clone)]
pub struct MACD {
    fast_ema: EMA,
    slow_ema: EMA,
    signal_ema: EMA,
    macd_line: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
pub struct MACDValue {
    pub macd: f64,
    pub signal: f64,
    pub histogram: f64,
}

impl MACD {
    pub fn new(fast_period: usize, slow_period: usize, signal_period: usize) -> Self {
        Self {
            fast_ema: EMA::new(fast_period),
            slow_ema: EMA::new(slow_period),
            signal_ema: EMA::new(signal_period),
            macd_line: None,
        }
    }
}

impl Default for MACD {
    fn default() -> Self {
        Self::new(12, 26, 9)
    }
}

impl Indicator for MACD {
    type Input = f64;
    type Output = MACDValue;

    fn update(&mut self, price: Self::Input) -> Option<Self::Output> {
        let fast = self.fast_ema.update(price)?;
        let slow = self.slow_ema.update(price)?;

        let macd = fast - slow;
        self.macd_line = Some(macd);

        let signal = self.signal_ema.update(macd)?;
        let histogram = macd - signal;

        Some(MACDValue {
            macd,
            signal,
            histogram,
        })
    }

    fn value(&self) -> Option<Self::Output> {
        let macd = self.macd_line?;
        let signal = self.signal_ema.value()?;
        let histogram = macd - signal;

        Some(MACDValue {
            macd,
            signal,
            histogram,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_macd() {
        let mut macd = MACD::default();

        for i in 1..=30 {
            let price = 100.0 + i as f64;
            let result = macd.update(price);
            if i >= 26 {
                assert!(result.is_some());
            }
        }
    }
}
