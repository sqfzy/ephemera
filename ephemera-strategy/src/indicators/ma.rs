use super::Indicator;
use std::collections::VecDeque;

/// 简单移动平均线 (Simple Moving Average)
#[derive(Debug, Clone)]
pub struct MA {
    period: usize,
    values: VecDeque<f64>,
    sum: f64,
}

impl MA {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            values: VecDeque::with_capacity(period),
            sum: 0.0,
        }
    }
}

impl Indicator for MA {
    type Input = f64;
    type Output = f64;

    fn update(&mut self, input: Self::Input) -> Option<Self::Output> {
        self.values.push_back(input);
        self.sum += input;

        if self.values.len() > self.period
            && let Some(old_value) = self.values.pop_front()
        {
            self.sum -= old_value;
        }

        if self.values.len() == self.period {
            Some(self.sum / self.period as f64)
        } else {
            None
        }
    }

    fn value(&self) -> Option<Self::Output> {
        if self.values.len() == self.period {
            Some(self.sum / self.period as f64)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ma() {
        let mut ma = MA::new(3);

        assert_eq!(ma.update(10.0), None);
        assert_eq!(ma.update(20.0), None);
        assert_eq!(ma.update(30.0), Some(20.0));
        assert_eq!(ma.update(40.0), Some(30.0));
    }
}
