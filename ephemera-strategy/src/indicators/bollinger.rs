use crate::indicators::MA;
use crate::Indicator;
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// 布林带指标
#[derive(Debug, Clone)]
pub struct BollingerBands {
    period: usize,
    std_dev_multiplier: Decimal,
    ma: MA,
    values: VecDeque<Decimal>,
}

#[derive(Debug, Clone, Copy)]
pub struct BollingerValue {
    pub upper: Decimal,
    pub middle: Decimal,
    pub lower: Decimal,
}

impl BollingerBands {
    pub fn new(period: usize, std_dev_multiplier: Decimal) -> Self {
        Self {
            period,
            std_dev_multiplier,
            ma: MA::new(period),
            values: VecDeque::with_capacity(period),
        }
    }

    /// 默认参数：周期20，标准差倍数2
    pub fn default() -> Self {
        Self::new(20, Decimal::TWO)
    }

    fn calculate_std_dev(&self, mean: Decimal) -> Decimal {
        if self.values.len() < self.period {
            return Decimal::ZERO;
        }

        let variance: Decimal = self.values
            .iter()
            .map(|&x| {
                let diff = x - mean;
                diff * diff
            })
            .sum::<Decimal>() / Decimal::from(self.period);

        // 简化的平方根计算（使用牛顿法）
        let mut sqrt = variance / Decimal::TWO;
        for _ in 0..10 {
            if sqrt.is_zero() {
                break;
            }
            sqrt = (sqrt + variance / sqrt) / Decimal::TWO;
        }
        sqrt
    }
}

impl Indicator for BollingerBands {
    type Input = Decimal;
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

    fn reset(&mut self) {
        self.ma.reset();
        self.values.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::dec;

    #[test]
    fn test_bollinger_bands() {
        let mut bb = BollingerBands::default();
        
        for i in 1..=25 {
            let price = dec!(100) + Decimal::from(i);
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
