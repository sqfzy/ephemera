use super::{Indicator, MA};
use std::collections::VecDeque;

/// Bollinger Bands - 布林带
///
/// # 原理
/// 布林带由 John Bollinger 在 1980 年代提出，是一个基于统计学的技术指标。
/// 它由三条线组成：中轨（移动平均线）和上下轨（中轨 ± N 倍标准差）。
/// 布林带会根据市场波动性自动调整宽度。
///
/// # 组成
/// - **中轨 (Middle Band)**: 通常是 20 期 SMA
/// - **上轨 (Upper Band)**: 中轨 + k × 标准差（通常 k=2）
/// - **下轨 (Lower Band)**: 中轨 - k × 标准差
///
/// # 解释
/// - **带宽收窄**: 波动性降低，可能预示即将出现大行情（突破）。
/// - **带宽扩张**: 波动性增加，趋势可能正在进行中。
/// - **价格触及上轨**: 可能超买，但在强趋势中可以沿上轨运行。
/// - **价格触及下轨**: 可能超卖，但在弱趋势中可以沿下轨运行。
/// - **布林带挤压 (Bollinger Squeeze)**: 带宽极度收窄，通常预示大波动即将来临。
/// - **走出布林带**: 价格突破上轨或下轨，可能是趋势加速信号。
#[derive(Debug, Clone)]
pub struct BollingerBands {
    period: usize,
    std_dev_multiplier: f64,
    ma: MA,
    values: VecDeque<f64>,
}

#[derive(Debug, Clone, Copy)]
pub struct BollingerBandsOutput {
    /// 中轨（移动平均线）
    pub middle: f64,
    /// 上轨（中轨 + k × 标准差）
    pub upper: f64,
    /// 下轨（中轨 - k × 标准差）
    pub lower: f64,
    /// 带宽百分比: (上轨 - 下轨) / 中轨 × 100
    pub bandwidth_pct: f64,
}

impl BollingerBands {
    pub fn new(period: usize, std_dev_multiplier: f64) -> Self {
        Self {
            period,
            std_dev_multiplier,
            ma: MA::new(period),
            values: VecDeque::with_capacity(period),
        }
    }

    /// 标准布林带 (20, 2)
    pub fn standard() -> Self {
        Self::new(20, 2.0)
    }

    /// 宽松布林带 (20, 2.5) - 减少假突破
    pub fn wide() -> Self {
        Self::new(20, 2.5)
    }

    /// 短期布林带 (10, 1.5)
    pub fn short_term() -> Self {
        Self::new(10, 1.5)
    }

    fn calculate_std_dev(&self, mean: f64) -> f64 {
        if self.values.is_empty() {
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
            / self.values.len() as f64;

        variance.sqrt()
    }
}

impl Indicator for BollingerBands {
    type Input = f64;
    type Output = Option<BollingerBandsOutput>;

    fn next_value(&mut self, input: Self::Input) -> Self::Output {
        // 1. 维护滑动窗口（先于 MA 更新，确保同步）
        self.values.push_back(input);

        if self.values.len() > self.period {
            self.values.pop_front();
        }

        // 2. 更新移动平均线
        let middle = self.ma.next_value(input)?;

        // 3. 确保有足够数据（此时 values 和 ma 应该是同步的）
        if self.values.len() < self.period {
            return None;
        }

        // 4. 计算标准差
        let std_dev = self.calculate_std_dev(middle);

        // 5. 计算上下轨
        let offset = std_dev * self.std_dev_multiplier;
        let upper = middle + offset;
        let lower = middle - offset;

        // 6. 计算带宽百分比
        let bandwidth_pct = if middle != 0.0 {
            ((upper - lower) / middle) * 100.0
        } else {
            0.0
        };

        Some(BollingerBandsOutput {
            middle,
            upper,
            lower,
            bandwidth_pct,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bollinger_bands_basic() {
        let mut bb = BollingerBands::new(3, 2.0);

        // 不够数据
        assert!(bb.next_value(10.0).is_none());
        assert!(bb.next_value(20.0).is_none());

        // 第三个值：prices = [10, 20, 30], mean = 20
        let result = bb.next_value(30.0);
        assert!(result.is_some());

        let output = result.unwrap();
        approx::assert_abs_diff_eq!(output.middle, 20.0);

        // 标准差 = sqrt(((10-20)^2 + (20-20)^2 + (30-20)^2) / 3)
        //        = sqrt((100 + 0 + 100) / 3)
        //        = sqrt(66.6666.. .) ≈ 8.165
        let expected_std = f64::sqrt((100.0 + 0.0 + 100.0) / 3.0);
        let expected_upper = 20.0 + 2.0 * expected_std;
        let expected_lower = 20.0 - 2.0 * expected_std;

        approx::assert_abs_diff_eq!(output.upper, expected_upper, epsilon = 0.01);
        approx::assert_abs_diff_eq!(output.lower, expected_lower, epsilon = 0.01);
    }

    #[test]
    fn test_bollinger_bands_rolling() {
        let mut bb = BollingerBands::new(3, 2.0);

        // 初始化
        bb.next_value(10.0);
        bb.next_value(20.0);
        bb.next_value(30.0);

        // 第四个值应该滚动窗口：[20, 30, 40]
        let result = bb.next_value(40.0);
        assert!(result.is_some());

        let output = result.unwrap();
        approx::assert_abs_diff_eq!(output.middle, 30.0); // (20+30+40)/3
    }

    #[test]
    fn test_bollinger_bands_low_volatility() {
        let mut bb = BollingerBands::new(5, 2.0);

        // 价格几乎不变（低波动）
        let prices = vec![100.0, 100.1, 99.9, 100.0, 100.1];

        let mut result = None;
        for price in prices {
            result = bb.next_value(price);
        }

        assert!(result.is_some());
        let output = result.unwrap();

        // 低波动时带宽应该很窄
        assert!(
            output.bandwidth_pct < 1.0,
            "Bandwidth should be narrow for low volatility, got {}",
            output.bandwidth_pct
        );
    }

    #[test]
    fn test_bollinger_bands_high_volatility() {
        let mut bb = BollingerBands::new(5, 2.0);

        // 价格大幅波动
        let prices = vec![100.0, 120.0, 80.0, 130.0, 70.0];

        let mut result = None;
        for price in prices {
            result = bb.next_value(price);
        }

        assert!(result.is_some());
        let output = result.unwrap();

        // 高波动时带宽应该很宽
        assert!(
            output.bandwidth_pct > 20.0,
            "Bandwidth should be wide for high volatility, got {}",
            output.bandwidth_pct
        );
    }

    #[test]
    fn test_bollinger_bands_zero_std_dev() {
        let mut bb = BollingerBands::new(3, 2.0);

        // 所有价格相同，标准差为 0
        bb.next_value(100.0);
        bb.next_value(100.0);
        let result = bb.next_value(100.0);

        assert!(result.is_some());
        let output = result.unwrap();

        approx::assert_abs_diff_eq!(output.middle, 100.0);
        approx::assert_abs_diff_eq!(output.upper, 100.0);
        approx::assert_abs_diff_eq!(output.lower, 100.0);
        approx::assert_abs_diff_eq!(output.bandwidth_pct, 0.0);
    }
}
