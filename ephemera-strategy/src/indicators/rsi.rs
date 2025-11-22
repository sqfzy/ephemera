use super::Indicator;
use std::collections::VecDeque;

/// 相对强弱指标 (Relative Strength Index)
#[derive(Debug, Clone)]
pub struct RSI {
    period: usize,
    gains: VecDeque<f64>,
    losses: VecDeque<f64>,
    prev_price: Option<f64>,
    avg_gain: f64,
    avg_loss: f64,
}

impl RSI {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            gains: VecDeque::with_capacity(period),
            losses: VecDeque::with_capacity(period),
            prev_price: None,
            avg_gain: 0.0,
            avg_loss: 0.0,
        }
    }

    fn calculate_rsi(&self) -> Option<f64> {
        if self.avg_loss == 0.0 {
            return Some(100.0);
        }

        let rs = self.avg_gain / self.avg_loss;
        Some(100.0 - (100.0 / (1.0 + rs)))
    }
}

impl Indicator for RSI {
    type Input = f64;
    type Output = f64;

    fn update(&mut self, price: Self::Input) -> Option<Self::Output> {
        let prev_price = match self.prev_price {
            Some(p) => p,
            None => {
                self.prev_price = Some(price);
                return None;
            }
        };

        let change = price - prev_price;
        let gain = if change > 0.0 {
            change
        } else {
            0.0
        };
        let loss = if change < 0.0 {
            -change
        } else {
            0.0
        };

        self.gains.push_back(gain);
        self.losses.push_back(loss);

        if self.gains.len() > self.period {
            self.gains.pop_front();
            self.losses.pop_front();
        }

        if self.gains.len() == self.period {
            self.avg_gain = self.gains.iter().sum::<f64>() / self.period as f64;
            self.avg_loss = self.losses.iter().sum::<f64>() / self.period as f64;
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
        self.avg_gain = 0.0;
        self.avg_loss = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rsi() {
        let mut rsi = RSI::new(14);

        // 需要至少15个数据点（prev + 14个period）
        for i in 1..=15 {
            let price = 100.0 + i as f64;
            let result = rsi.update(price);
            if i == 15 {
                assert!(result.is_some());
            }
        }
    }
}
