use super::Indicator;
use std::collections::VecDeque;

/// MVRV Z-Score (滚动窗口版)
///
/// # 什么是 MVRV?
/// MVRV (Market Value to Realized Value) 是市值与已实现市值的比率。
/// - **MV (市值)**: 当前价格 * 流通量
/// - **RV (已实现市值)**: 基于链上最后一次移动时的价格计算的总价值（近似看作全市场的平均持仓成本）。
///
/// # 什么是 Z-Score?
/// Z-Score (标准分数) 衡量数据点偏离平均值的程度（以标准差为单位）。
///
/// # 指标逻辑
/// 本指标计算 MVRV 比率，并在一个滚动的 `period` 窗口内计算该比率的 Z-Score。
/// 这有助于判断当前的 MVRV 比率相对于过去 `period` 天是异常高（高估）还是异常低（低估）。
///
/// # 信号解读
/// - **Z-Score > 2.0**: 红色区域。MVRV 显著高于均值，市场可能过热（顶部风险）。
/// - **Z-Score < -1.0**: 绿色区域。MVRV 显著低于均值，市场可能低估（底部机会）。
/// - **0 附近**: 市场处于该周期内的平均水平。
#[derive(Debug, Clone)]
pub struct MVRVZScore {
    period: usize,
    mvrv_values: VecDeque<f64>,
    sum: f64,
    sum_squared: f64,
}

impl MVRVZScore {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            mvrv_values: VecDeque::with_capacity(period),
            sum: 0.0,
            sum_squared: 0.0,
        }
    }

    fn calculate_mean(&self) -> f64 {
        self.sum / self.mvrv_values.len() as f64
    }

    fn calculate_std_dev(&self, mean: f64) -> f64 {
        let variance = (self.sum_squared / self.mvrv_values.len() as f64) - (mean * mean);
        variance.sqrt()
    }
}

impl Indicator for MVRVZScore {
    type Input = (f64, f64); // (market_cap, realized_cap)
    type Output = Option<f64>;

    /// Input: (market_cap, realized_cap)
    fn next_value(&mut self, input: Self::Input) -> Self::Output {
        let (market_cap, realized_cap) = input;

        // Calculate MVRV ratio
        if realized_cap == 0.0 {
            return None;
        }

        let mvrv = market_cap / realized_cap;

        // Add new MVRV value
        self.mvrv_values.push_back(mvrv);
        self.sum += mvrv;
        self.sum_squared += mvrv * mvrv;

        // Remove old value if exceeded period
        if self.mvrv_values.len() > self.period
            && let Some(old_value) = self.mvrv_values.pop_front()
        {
            self.sum -= old_value;
            self.sum_squared -= old_value * old_value;
        }

        // Calculate Z-Score when we have enough data
        if self.mvrv_values.len() == self.period {
            let mean = self.calculate_mean();
            let std_dev = self.calculate_std_dev(mean);

            if std_dev == 0.0 {
                return Some(0.0);
            }

            let z_score = (mvrv - mean) / std_dev;
            Some(z_score)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mvrv_zscore() {
        let mut mvrv_zscore = MVRVZScore::new(3);

        // Not enough data yet
        assert!(mvrv_zscore.next_value((100.0, 100.0)).is_none());
        assert!(mvrv_zscore.next_value((110.0, 100.0)).is_none());

        // Now we have 3 values:  MVRV = [1.0, 1.1, 1.2]
        let result = mvrv_zscore.next_value((120.0, 100.0));
        assert!(result.is_some());

        // Mean = (1.0 + 1.1 + 1.2) / 3 = 1.1
        // Current MVRV = 1.2
        // Z-score should be positive
        let z_score = result.unwrap();
        assert!(z_score > 0.0);

        // Add another value, rolling window
        let result = mvrv_zscore.next_value((130.0, 100.0));
        assert!(result.is_some());
    }

    #[test]
    fn test_mvrv_zscore_zero_realized_cap() {
        let mut mvrv_zscore = MVRVZScore::new(3);

        // Should return None when realized_cap is 0
        assert!(mvrv_zscore.next_value((100.0, 0.0)).is_none());
    }

    #[test]
    fn test_mvrv_zscore_detailed() {
        let mut mvrv_zscore = MVRVZScore::new(3);

        // MVRV values will be:  1.0, 1.0, 1.0
        assert!(mvrv_zscore.next_value((100.0, 100.0)).is_none());
        assert!(mvrv_zscore.next_value((100.0, 100.0)).is_none());
        let result = mvrv_zscore.next_value((100.0, 100.0));

        // All values are the same, so std_dev = 0, z_score = 0
        assert!(result.is_some());
        approx::assert_abs_diff_eq!(result.unwrap(), 0.0, epsilon = 1e-10);
    }
}

