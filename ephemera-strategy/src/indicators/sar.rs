use super::Indicator;

/// Parabolic SAR (Stop and Reverse) 指标
/// 用于确定趋势方向和潜在的反转点
#[derive(Debug, Clone)]
pub struct SAR {
    /// 加速因子的初始值
    af_start: f64,
    /// 加速因子的增量
    af_increment: f64,
    /// 加速因子的最大值
    af_max: f64,
    /// 当前加速因子
    af: f64,
    /// 当前SAR值
    sar: Option<f64>,
    /// 极值点（Extreme Point）
    ep: Option<f64>,
    /// 当前趋势：true为上涨，false为下跌
    is_uptrend: bool,
    /// 前一个最高价
    prev_high: Option<f64>,
    /// 前一个最低价
    prev_low: Option<f64>,
    /// 当前K线的最高价
    current_high: Option<f64>,
    /// 当前K线的最低价
    current_low: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
pub struct SARValue {
    /// SAR值
    pub sar: f64,
    /// 是否为上涨趋势
    pub is_uptrend: bool,
}

impl SAR {
    pub fn new(af_start: f64, af_increment: f64, af_max: f64) -> Self {
        Self {
            af_start,
            af_increment,
            af_max,
            af: af_start,
            sar: None,
            ep: None,
            is_uptrend: true,
            prev_high: None,
            prev_low: None,
            current_high: None,
            current_low: None,
        }
    }

    /// 更新SAR指标（需要提供K线的高低价）
    pub fn update_with_hl(&mut self, high: f64, low: f64) -> Option<SARValue> {
        // 初始化
        if self.sar.is_none() {
            self.sar = Some(low);
            self.ep = Some(high);
            self.prev_high = Some(high);
            self.prev_low = Some(low);
            self.is_uptrend = true;
            self.current_high = Some(high);
            self.current_low = Some(low);
            return Some(SARValue {
                sar: low,
                is_uptrend: true,
            });
        }

        let prev_sar = self.sar.unwrap();
        let prev_ep = self.ep.unwrap();

        // 计算新的SAR
        let mut new_sar = prev_sar + self.af * (prev_ep - prev_sar);

        // 更新极值点和加速因子
        if self.is_uptrend {
            // 上涨趋势
            // SAR不能高于前两根K线的最低价
            if let Some(prev_low) = self.prev_low {
                new_sar = new_sar.min(prev_low);
            }
            if let Some(current_low) = self.current_low {
                new_sar = new_sar.min(current_low);
            }

            // 检查是否发生趋势反转
            if low < new_sar {
                // 趋势反转为下跌
                self.is_uptrend = false;
                new_sar = prev_ep; // SAR变为之前的极值点
                self.ep = Some(low); // EP变为当前最低价
                self.af = self.af_start; // 重置加速因子
            } else {
                // 继续上涨趋势
                if high > prev_ep {
                    // 创新高，更新EP和AF
                    self.ep = Some(high);
                    self.af = (self.af + self.af_increment).min(self.af_max);
                }
            }
        } else {
            // 下跌趋势
            // SAR不能低于前两根K线的最高价
            if let Some(prev_high) = self.prev_high {
                new_sar = new_sar.max(prev_high);
            }
            if let Some(current_high) = self.current_high {
                new_sar = new_sar.max(current_high);
            }

            // 检查是否发生趋势反转
            if high > new_sar {
                // 趋势反转为上涨
                self.is_uptrend = true;
                new_sar = prev_ep; // SAR变为之前的极值点
                self.ep = Some(high); // EP变为当前最高价
                self.af = self.af_start; // 重置加速因子
            } else {
                // 继续下跌趋势
                if low < prev_ep {
                    // 创新低，更新EP和AF
                    self.ep = Some(low);
                    self.af = (self.af + self.af_increment).min(self.af_max);
                }
            }
        }

        // 更新状态
        self.prev_high = self.current_high;
        self.prev_low = self.current_low;
        self.current_high = Some(high);
        self.current_low = Some(low);
        self.sar = Some(new_sar);

        Some(SARValue {
            sar: new_sar,
            is_uptrend: self.is_uptrend,
        })
    }
}

impl Default for SAR {
    fn default() -> Self {
        // 标准参数：起始0.02，增量0.02，最大0.2
        Self::new(0.02, 0.02, 0.2)
    }
}

impl Indicator for SAR {
    type Input = (f64, f64); // (high, low)
    type Output = SARValue;

    fn update(&mut self, input: Self::Input) -> Option<Self::Output> {
        let (high, low) = input;
        self.update_with_hl(high, low)
    }

    fn value(&self) -> Option<Self::Output> {
        self.sar.map(|sar| SARValue {
            sar,
            is_uptrend: self.is_uptrend,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sar_initialization() {
        let mut sar = SAR::default();

        let result = sar.update_with_hl(100.0, 98.0);
        assert!(result.is_some());
        let val = result.unwrap();
        assert_eq!(val.sar, 98.0);
        assert!(val.is_uptrend);
    }

    #[test]
    fn test_sar_uptrend() {
        let mut sar = SAR::default();

        // 模拟上涨趋势
        let prices = [
            (100.0, 98.0),
            (102.0, 99.0),
            (104.0, 101.0),
            (106.0, 103.0),
            (108.0, 105.0),
        ];

        for (i, &(high, low)) in prices.iter().enumerate() {
            let result = sar.update_with_hl(high, low);
            assert!(result.is_some());
            let val = result.unwrap();

            if i == 0 {
                assert_eq!(val.sar, 98.0);
            }
            // 上涨趋势中，SAR应该在价格下方
            assert!(val.sar < low || i == 0);
        }
    }

    #[test]
    fn test_sar_trend_reversal() {
        let mut sar = SAR::default();

        // 先上涨
        sar.update_with_hl(100.0, 98.0);
        sar.update_with_hl(102.0, 99.0);
        sar.update_with_hl(104.0, 101.0);

        // 然后下跌，触发反转
        let result = sar.update_with_hl(100.0, 95.0);
        assert!(result.is_some());
        let val = result.unwrap();

        // 应该检测到趋势反转
        assert!(!val.is_uptrend);
    }

    #[test]
    fn test_sar_with_indicator_trait() {
        let mut sar = SAR::default();

        let result1 = sar.update((100.0, 98.0));
        assert!(result1.is_some());

        let result2 = sar.update((102.0, 99.0));
        assert!(result2.is_some());

        let current = sar.value();
        assert!(current.is_some());
    }
}
