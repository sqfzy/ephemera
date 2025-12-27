use super::Indicator;

/// EMA - 指数移动平均线 (Exponential Moving Average)
///
/// # 原理
/// EMA 是一种加权移动平均线，它对近期价格赋予更高的权重。
/// 与简单移动平均（SMA）相比，EMA 对价格变化的反应更快，更贴近当前市场状态。
///
/// # 公式
/// ```text
/// EMA(t) = Price(t) × α + EMA(t-1) × (1 - α)
/// 其中:  α = 2 / (period + 1)  (平滑因子)
/// ```
///
/// # 解释
/// - **快速响应**: EMA 比 SMA 更快地反映价格变化。
/// - **趋势跟踪**: 价格在 EMA 上方为上升趋势，下方为下降趋势。
/// - **支撑阻力**: EMA 可作为动态支撑或阻力位。
/// - **交叉策略**: 短期 EMA 上穿长期 EMA 为金叉（买入信号），下穿为死叉（卖出信号）。
#[derive(Debug, Clone)]
pub struct EMA {
    period: usize,
    alpha: f64,
    current_ema: Option<f64>,
    init_values: Vec<f64>,
}

impl EMA {
    pub fn new(period: usize) -> Self {
        let alpha = 2.0 / (period as f64 + 1.0);
        Self {
            period,
            alpha,
            current_ema: None,
            init_values: Vec::with_capacity(period),
        }
    }

    /// MACD 快线
    pub fn ema12() -> Self {
        Self::new(12)
    }

    /// 短期趋势
    pub fn ema20() -> Self {
        Self::new(20)
    }

    /// MACD 慢线
    pub fn ema26() -> Self {
        Self::new(26)
    }

    /// 中期趋势
    pub fn ema50() -> Self {
        Self::new(50)
    }

    /// 长期趋势（牛熊线）
    pub fn ema200() -> Self {
        Self::new(200)
    }
}

impl Indicator for EMA {
    type Input = f64;
    type Output = Option<f64>;

    fn next_value(&mut self, input: Self::Input) -> Self::Output {
        match self.current_ema {
            None => {
                // 初始化阶段：使用 SMA 作为第一个 EMA 值
                self.init_values.push(input);

                if self.init_values.len() == self.period {
                    let sma: f64 = self.init_values.iter().sum::<f64>() / self.period as f64;
                    self.current_ema = Some(sma);
                    Some(sma)
                } else {
                    None
                }
            }
            Some(prev_ema) => {
                // 使用 EMA 公式计算
                let new_ema = input * self.alpha + prev_ema * (1.0 - self.alpha);
                self.current_ema = Some(new_ema);
                Some(new_ema)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ema_initialization() {
        let mut ema = EMA::new(3);

        // 前两个值应该返回 None
        assert!(ema.next_value(10.0).is_none());
        assert!(ema.next_value(20.0).is_none());

        // 第三个值应该返回 SMA
        let result = ema.next_value(30.0);
        assert!(result.is_some());
        approx::assert_abs_diff_eq!(result.unwrap(), 20.0); // (10+20+30)/3
    }

    #[test]
    fn test_ema_calculation() {
        let mut ema = EMA::new(3);

        // 初始化:  [10, 20, 30]
        ema.next_value(10.0);
        ema.next_value(20.0);
        let first_ema = ema.next_value(30.0).unwrap();
        approx::assert_abs_diff_eq!(first_ema, 20.0); // SMA = (10+20+30)/3 = 20

        // α = 2/(3+1) = 0.5
        // 下一个 EMA = 40 * 0.5 + 20 * 0.5 = 30
        let second_ema = ema.next_value(40.0).unwrap();
        approx::assert_abs_diff_eq!(second_ema, 30.0); // 40*0.5 + 20*0.5 = 30

        // 再下一个 EMA = 50 * 0.5 + 30 * 0.5 = 40
        let third_ema = ema.next_value(50.0).unwrap();
        approx::assert_abs_diff_eq!(third_ema, 40.0); // 50*0.5 + 30*0.5 = 40
    }

    #[test]
    fn test_ema_vs_sma_responsiveness() {
        let mut ema = EMA::new(5);
        let prices = vec![10.0, 11.0, 12.0, 13.0, 14.0, 20.0]; // 突然跳涨

        let mut last_value = None;
        for price in prices {
            if let Some(val) = ema.next_value(price) {
                last_value = Some(val);
            }
        }

        // EMA 应该更接近最新价格 20.0
        assert!(last_value.is_some());
        let ema_value = last_value.unwrap();

        // 第一个 EMA = (10+11+12+13+14)/5 = 12
        // α = 2/(5+1) = 0.333...
        // 第二个 EMA = 20 * 0.333... + 12 * 0.666... ≈ 14.67
        //
        // SMA(5) 在最后会是 (11+12+13+14+20)/5 = 14
        // EMA 应该略高于 SMA
        assert!(
            ema_value > 14.0,
            "EMA should be more responsive, got {}",
            ema_value
        );
        assert!(
            ema_value < 16.0,
            "EMA should still be reasonable, got {}",
            ema_value
        );
    }

    #[test]
    fn test_ema_with_constant_prices() {
        let mut ema = EMA::new(3);

        // 所有价格都是 100
        ema.next_value(100.0);
        ema.next_value(100.0);
        let first = ema.next_value(100.0).unwrap();
        approx::assert_abs_diff_eq!(first, 100.0);

        let second = ema.next_value(100.0).unwrap();
        approx::assert_abs_diff_eq!(second, 100.0);

        let third = ema.next_value(100.0).unwrap();
        approx::assert_abs_diff_eq!(third, 100.0);
    }

    #[test]
    fn test_ema_different_periods() {
        let mut ema_short = EMA::new(3);
        let mut ema_long = EMA::new(5);

        let prices = vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0];

        let mut short_result = None;
        let mut long_result = None;

        for price in prices {
            if let Some(val) = ema_short.next_value(price) {
                short_result = Some(val);
            }
            if let Some(val) = ema_long.next_value(price) {
                long_result = Some(val);
            }
        }

        // 短周期 EMA 应该更接近最新价格
        assert!(short_result.is_some());
        assert!(long_result.is_some());

        let short = short_result.unwrap();
        let long = long_result.unwrap();

        // 短周期应该更高（因为价格在上涨）
        assert!(
            short > long,
            "Short EMA ({}) should be > Long EMA ({}) in uptrend",
            short,
            long
        );
    }
}
