use super::Indicator;
use rust_decimal::Decimal;

/// 指数移动平均线 (Exponential Moving Average)
#[derive(Debug, Clone)]
pub struct EMA {
    period: usize,
    multiplier: Decimal,
    value: Option<Decimal>,
}

impl EMA {
    pub fn new(period: usize) -> Self {
        let multiplier = Decimal::TWO / Decimal::from(period + 1);
        Self {
            period,
            multiplier,
            value: None,
        }
    }
}

impl Indicator for EMA {
    type Input = Decimal;
    type Output = Decimal;

    fn update(&mut self, input: Self::Input) -> Option<Self::Output> {
        self.value = Some(match self.value {
            None => input,
            Some(prev_ema) => {
                input * self.multiplier + prev_ema * (Decimal::ONE - self.multiplier)
            }
        });
        self.value
    }

    fn value(&self) -> Option<Self::Output> {
        self.value
    }

    fn reset(&mut self) {
        self.value = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::dec;

    #[test]
    fn test_ema() {
        let mut ema = EMA::new(3);
        
        let v1 = ema.update(dec!(10)).unwrap();
        assert_eq!(v1, dec!(10));
        
        let v2 = ema.update(dec!(20));
        assert!(v2.is_some());
    }
}
