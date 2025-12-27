use super::Indicator;
use std::collections::VecDeque;

/// 简单移动平均线 (Simple Moving Average, SMA)
///
/// # 原理
/// SMA 是最基本的趋势跟踪指标。它通过计算过去 N 个周期内价格的算术平均值，
/// 来平滑价格波动，从而过滤掉短期的噪音，展示价格的长期趋势。
///
/// # 解释
/// - **上升趋势**: 当价格位于 MA 之上，且 MA 向上倾斜时。
/// - **下降趋势**: 当价格位于 MA 之下，且 MA 向下倾斜时。
/// - **支撑/阻力**: MA 常被视作动态的支撑线或阻力线。
///
/// # 常见参数
/// - **MA20**: 短期趋势，布林带的中轨通常使用此参数。
/// - **MA50**: 中期趋势，常用于判断中期调整。
/// - **MA200**: 长期趋势，著名的 "牛熊分界线"。
#[derive(Debug, Clone)]
pub struct MA {
    pub(crate) period: usize,
    pub(crate) values: VecDeque<f64>,
    pub(crate) sum: f64,
}

impl MA {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            values: VecDeque::with_capacity(period),
            sum: 0.0,
        }
    }

    /// 周线级别短期趋势
    pub fn ma7() -> Self {
        Self::new(7)
    }

    /// 布林带中轨 / 短期生命线
    pub fn ma20() -> Self {
        Self::new(20)
    }

    /// 中期趋势 / 强弱分界
    pub fn ma50() -> Self {
        Self::new(50)
    }

    /// 半年线
    pub fn ma120() -> Self {
        Self::new(120)
    }

    /// 牛熊分界线 / 长期趋势
    pub fn ma200() -> Self {
        Self::new(200)
    }
}

impl Indicator for MA {
    type Input = f64;
    type Output = Option<f64>;

    fn on_data(&mut self, input: Self::Input) -> Self::Output {
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
}

#[test]
fn test_ma() {
    let mut ma = MA::new(3);

    assert!(ma.on_data(10.0).is_none());
    assert!(ma.on_data(20.0).is_none());
    approx::assert_abs_diff_eq!(ma.on_data(30.0).unwrap(), 20.0);
    approx::assert_abs_diff_eq!(ma.on_data(40.0).unwrap(), 30.0);
}
