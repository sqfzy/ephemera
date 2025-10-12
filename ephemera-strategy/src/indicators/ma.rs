use crate::Indicator;
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// 简单移动平均线 (Simple Moving Average)
#[derive(Debug, Clone)]
pub struct MA {
    period: usize,
    values: VecDeque<Decimal>,
    sum: Decimal,
}

impl MA {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            values: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        }
    }
}

impl Indicator for MA {
    type Input = Decimal;
    type Output = Decimal;

    fn update(&mut self, input: Self::Input) -> Option<Self::Output> {
        self.values.push_back(input);
        self.sum += input;

        if self.values.len() > self.period {
            if let Some(old_value) = self.values.pop_front() {
                self.sum -= old_value;
            }
        }

        if self.values.len() == self.period {
            Some(self.sum / Decimal::from(self.period))
        } else {
            None
        }
    }

    fn value(&self) -> Option<Self::Output> {
        if self.values.len() == self.period {
            Some(self.sum / Decimal::from(self.period))
        } else {
            None
        }
    }

    fn reset(&mut self) {
        self.values.clear();
        self.sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::dec;

    #[test]
    fn test_ma() {
        let mut ma = MA::new(3);
        
        assert_eq!(ma.update(dec!(10)), None);
        assert_eq!(ma.update(dec!(20)), None);
        assert_eq!(ma.update(dec!(30)), Some(dec!(20)));
        assert_eq!(ma.update(dec!(40)), Some(dec!(30)));
    }
}
