use super::Indicator;
use std::collections::VecDeque;

/// RSI - 相对强弱指标 (Relative Strength Index)
///
/// # 原理
/// RSI 是由 J. Welles Wilder 在 1978 年提出的动量震荡指标，用于衡量价格变动的速度和幅度。
/// 它通过比较一段时期内的平均上涨幅度和平均下跌幅度来判断市场的超买超卖状态。
///
/// # 公式
/// ```text
/// RS = 平均涨幅 / 平均跌幅
/// RSI = 100 - (100 / (1 + RS))
/// ```
///
/// # 解释
/// - **RSI > 70**: **超买区域**。价格可能过高，存在回调风险。
/// - **RSI < 30**: **超卖区域**。价格可能过低，存在反弹机会。
/// - **50 附近**: 市场处于平衡状态。
/// - **背离**: 价格创新高但 RSI 未创新高（顶背离），或价格创新低但 RSI 未创新低（底背离），可能预示反转。
#[derive(Debug, Clone)]
pub struct RSI {
    period: usize,
    price_changes: VecDeque<f64>,
    last_price: Option<f64>,
    avg_gain: f64,
    avg_loss: f64,
    is_initialized: bool,
}

impl RSI {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            price_changes: VecDeque::with_capacity(period),
            last_price: None,
            avg_gain: 0.0,
            avg_loss: 0.0,
            is_initialized: false,
        }
    }

    /// 短线交易
    pub fn rsi9() -> Self {
        Self::new(9)
    }

    /// 标准周期（最常用）
    pub fn rsi14() -> Self {
        Self::new(14)
    }

    /// 中长线分析
    pub fn rsi25() -> Self {
        Self::new(25)
    }

    fn calculate_initial_averages(&mut self) -> Option<f64> {
        if self.price_changes.len() < self.period {
            return None;
        }

        let mut sum_gain = 0.0;
        let mut sum_loss = 0.0;

        for &change in &self.price_changes {
            if change > 0.0 {
                sum_gain += change;
            } else {
                sum_loss += -change;
            }
        }

        self.avg_gain = sum_gain / self.period as f64;
        self.avg_loss = sum_loss / self.period as f64;
        self.is_initialized = true;

        self.calculate_rsi()
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
    type Output = Option<f64>;

    fn next_value(&mut self, input: Self::Input) -> Self::Output {
        let current_price = input;

        // 计算价格变化
        let price_change = if let Some(last) = self.last_price {
            current_price - last
        } else {
            self.last_price = Some(current_price);
            return None;
        };

        self.last_price = Some(current_price);

        if !self.is_initialized {
            // 初始化阶段：收集足够的数据
            self.price_changes.push_back(price_change);

            if self.price_changes.len() == self.period {
                return self.calculate_initial_averages();
            }

            None
        } else {
            // 使用 Wilder's smoothing method (类似 EMA)
            let gain = if price_change > 0.0 {
                price_change
            } else {
                0.0
            };
            let loss = if price_change < 0.0 {
                -price_change
            } else {
                0.0
            };

            self.avg_gain = (self.avg_gain * (self.period - 1) as f64 + gain) / self.period as f64;
            self.avg_loss = (self.avg_loss * (self.period - 1) as f64 + loss) / self.period as f64;

            self.calculate_rsi()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rsi_basic() {
        let mut rsi = RSI::new(14);

        // 价格持续上涨应该产生高 RSI
        let prices = vec![
            44.0, 44.25, 44.37, 44.50, 44.75, 45.00, 45.25, 45.50, 45.75, 46.00, 46.25, 46.50,
            46.75, 47.00, 47.25,
        ];

        let mut result = None;
        for price in prices {
            result = rsi.next_value(price);
        }

        assert!(result.is_some());
        let rsi_value = result.unwrap();

        // 持续上涨应该产生高 RSI（接近或超过 70）
        assert!(
            rsi_value > 60.0,
            "RSI should be high for uptrend, got {}",
            rsi_value
        );
    }

    #[test]
    fn test_rsi_oversold() {
        let mut rsi = RSI::new(14);

        // 价格持续下跌应该产生低 RSI
        let prices = vec![
            50.0, 49.5, 49.0, 48.5, 48.0, 47.5, 47.0, 46.5, 46.0, 45.5, 45.0, 44.5, 44.0, 43.5,
            43.0,
        ];

        let mut result = None;
        for price in prices {
            result = rsi.next_value(price);
        }

        assert!(result.is_some());
        let rsi_value = result.unwrap();

        // 持续下跌应该产生低 RSI（接近或低于 30）
        assert!(
            rsi_value < 40.0,
            "RSI should be low for downtrend, got {}",
            rsi_value
        );
    }
}
