use super::Indicator;

/// 指数移动平均线 (Exponential Moving Average)
#[derive(Debug, Clone)]
pub struct EMA {
    multiplier: f64,
    value: Option<f64>,
}

impl EMA {
    pub fn new(period: usize) -> Self {
        let multiplier = 2.0 / (period + 1) as f64;
        Self {
            multiplier,
            value: None,
        }
    }
}

impl Indicator for EMA {
    type Input = f64;
    type Output = f64;

    fn update(&mut self, input: Self::Input) -> Option<Self::Output> {
        self.value = Some(match self.value {
            None => input,
            Some(prev_ema) => input * self.multiplier + prev_ema * (1.0 - self.multiplier),
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

    #[test]
    fn test_ema() {
        let mut ema = EMA::new(3);

        let v1 = ema.update(10.0).unwrap();
        assert_eq!(v1, 10.0);

        let v2 = ema.update(20.0);
        assert!(v2.is_some());
    }
}
