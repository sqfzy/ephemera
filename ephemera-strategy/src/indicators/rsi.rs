use super::Indicator;
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// 相对强弱指标 (Relative Strength Index)
#[derive(Debug, Clone)]
pub struct RSI {
    period: usize,
    gains: VecDeque<Decimal>,
    losses: VecDeque<Decimal>,
    prev_price: Option<Decimal>,
    avg_gain: Decimal,
    avg_loss: Decimal,
}

impl RSI {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            gains: VecDeque::with_capacity(period),
            losses: VecDeque::with_capacity(period),
            prev_price: None,
            avg_gain: Decimal::ZERO,
            avg_loss: Decimal::ZERO,
        }
    }

    fn calculate_rsi(&self) -> Option<Decimal> {
        if self.avg_loss.is_zero() {
            return Some(Decimal::from(100));
        }

        let rs = self.avg_gain / self.avg_loss;
        Some(Decimal::from(100) - (Decimal::from(100) / (Decimal::ONE + rs)))
    }
}

impl Indicator for RSI {
    type Input = Decimal;
    type Output = Decimal;

    fn update(&mut self, price: Self::Input) -> Option<Self::Output> {
        let prev_price = match self.prev_price {
            Some(p) => p,
            None => {
                self.prev_price = Some(price);
                return None;
            }
        };

        let change = price - prev_price;
        let gain = if change > Decimal::ZERO {
            change
        } else {
            Decimal::ZERO
        };
        let loss = if change < Decimal::ZERO {
            -change
        } else {
            Decimal::ZERO
        };

        self.gains.push_back(gain);
        self.losses.push_back(loss);

        if self.gains.len() > self.period {
            self.gains.pop_front();
            self.losses.pop_front();
        }

        if self.gains.len() == self.period {
            self.avg_gain = self.gains.iter().sum::<Decimal>() / Decimal::from(self.period);
            self.avg_loss = self.losses.iter().sum::<Decimal>() / Decimal::from(self.period);
        }

        self.prev_price = Some(price);

        if self.gains.len() == self.period {
            self.calculate_rsi()
        } else {
            None
        }
    }

    fn value(&self) -> Option<Self::Output> {
        if self.gains.len() == self.period {
            self.calculate_rsi()
        } else {
            None
        }
    }

    fn reset(&mut self) {
        self.gains.clear();
        self.losses.clear();
        self.prev_price = None;
        self.avg_gain = Decimal::ZERO;
        self.avg_loss = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::dec;

    #[test]
    fn test_rsi() {
        let mut rsi = RSI::new(14);

        // 需要至少15个数据点（prev + 14个period）
        for i in 1..=15 {
            let price = dec!(100) + Decimal::from(i);
            let result = rsi.update(price);
            if i == 15 {
                assert!(result.is_some());
            }
        }
    }
}
