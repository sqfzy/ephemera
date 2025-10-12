use crate::indicators::EMA;
use crate::Indicator;
use rust_decimal::Decimal;

/// MACD指标
#[derive(Debug, Clone)]
pub struct MACD {
    fast_ema: EMA,
    slow_ema: EMA,
    signal_ema: EMA,
    macd_line: Option<Decimal>,
}

#[derive(Debug, Clone, Copy)]
pub struct MACDValue {
    pub macd: Decimal,
    pub signal: Decimal,
    pub histogram: Decimal,
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

    /// 默认参数：12, 26, 9
    pub fn default() -> Self {
        Self::new(12, 26, 9)
    }
}

impl Indicator for MACD {
    type Input = Decimal;
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

    fn reset(&mut self) {
        self.fast_ema.reset();
        self.slow_ema.reset();
        self.signal_ema.reset();
        self.macd_line = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::dec;

    #[test]
    fn test_macd() {
        let mut macd = MACD::default();
        
        for i in 1..=30 {
            let price = dec!(100) + Decimal::from(i);
            let result = macd.update(price);
            if i >= 26 {
                assert!(result.is_some());
            }
        }
    }
}
