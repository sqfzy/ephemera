use super::Indicator;
use crate::indicators::MA;
use std::collections::VecDeque;

/// 布林带指标
#[derive(Debug, Clone)]
pub struct BB {
    period: usize,
    std_dev_multiplier: f64,
    ma: MA,
    values: VecDeque<f64>,
}

#[derive(Debug, Clone, Copy)]
pub struct BollingerValue {
    pub upper: f64,
    pub middle: f64,
    pub lower: f64,
}

impl BB {
    pub fn new(period: usize, std_dev_multiplier: f64) -> Self {
        Self {
            period,
            std_dev_multiplier,
            ma: MA::new(period),
            values: VecDeque::with_capacity(period),
        }
    }

    fn calculate_std_dev(&self, mean: f64) -> f64 {
        if self.values.len() < self.period {
            return 0.0;
        }

        let variance: f64 = self
            .values
            .iter()
            .map(|&x| {
                let diff = x - mean;
                diff * diff
            })
            .sum::<f64>()
            / self.period as f64;

        // 简化的平方根计算（使用牛顿法）
        let mut sqrt = variance / 2.0;
        for _ in 0..10 {
            if sqrt == 0.0 {
                break;
            }
            sqrt = (sqrt + variance / sqrt) / 2.0;
        }
        sqrt
    }
}

impl Default for BB {
    fn default() -> Self {
        Self::new(20, 2.0)
    }
}

impl Indicator for BB {
    type Input = f64;
    type Output = BollingerValue;

    fn update(&mut self, price: Self::Input) -> Option<Self::Output> {
        self.values.push_back(price);
        if self.values.len() > self.period {
            self.values.pop_front();
        }

        let middle = self.ma.update(price)?;
        let std_dev = self.calculate_std_dev(middle);
        let band_width = std_dev * self.std_dev_multiplier;

        Some(BollingerValue {
            upper: middle + band_width,
            middle,
            lower: middle - band_width,
        })
    }

    fn value(&self) -> Option<Self::Output> {
        let middle = self.ma.value()?;
        let std_dev = self.calculate_std_dev(middle);
        let band_width = std_dev * self.std_dev_multiplier;

        Some(BollingerValue {
            upper: middle + band_width,
            middle,
            lower: middle - band_width,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bollinger_bands() {
        let mut bb = BB::default();

        for i in 1..=25 {
            let price = 100.0 + i as f64;
            let result = bb.update(price);
            if i >= 20 {
                assert!(result.is_some());
                let val = result.unwrap();
                assert!(val.upper > val.middle);
                assert!(val.middle > val.lower);
            }
        }
    }
}
