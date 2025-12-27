use super::Indicator;
use crate::indicators::MA;

pub const BTC_GENESIS_TIMESTAMP: u64 = 1_230_912_000; // 2009-01-03 00:00:00 UTC
pub const ETH_GENESIS_TIMESTAMP: u64 = 1_438_269_973; // 2015-07-30 15:26:13 UTC
pub const LTC_GENESIS_TIMESTAMP: u64 = 1_317_972_665; // 2011-10-07 07:31:05 UTC
pub const BCH_GENESIS_TIMESTAMP: u64 = 1_501_593_374; // 2017-08-01 13:16:14 UTC
pub const DOGE_GENESIS_TIMESTAMP: u64 = 1_386_325_540; // 2013-12-06 10:25:40 UTC

const SECONDS_PER_DAY: u64 = 86400;

/// AHR999 囤币指标 (AHR999 Index)
///
/// # 简介
/// AHR999 是由微博用户 @ahr999 创建的辅助比特币定投和抄底的指标。
/// 它隐含了比特币价格与“200日定投成本”和“指数增长估值”的长期关系。
///
/// # 公式
/// `AHR = (价格 / 200日定投成本) * (价格 / 指数增长估值)`
///
/// # 指标解读 (经典参考)
/// - **AHR < 0.45**: **抄底区间**。价格被极度低估，建议加大买入。
/// - **0.45 < AHR < 1.2**: **定投区间**。价格处于合理范围，适合坚持定投。
/// - **AHR > 1.2**: **起飞区间**。价格相对较高，不建议大规模买入，可以观望或持有。
///
/// # 注意
/// 该指标高度依赖指数增长模型的参数（斜率 `slope` 和 截距 `intercept`）。
/// 不同的历史时期拟合出的参数可能不同。
#[derive(Debug, Clone)]
pub struct AHR {
    pub(crate) ma200: MA,
    /// 创世时间戳 (秒)
    pub(crate) genesis_ts: u64,
    /// 指数增长斜率
    pub(crate) slope: f64,
    /// 预计算的截距因子: 10^intercept
    pub(crate) intercept_factor: f64,
}

impl AHR {
    pub fn new(genesis_ts: u64, slope: f64, intercept: f64) -> Self {
        Self {
            ma200: MA::new(200),
            genesis_ts,
            slope,
            intercept_factor: 10_f64.powf(intercept),
        }
    }

    /// 币龄（天）
    #[inline]
    fn calculate_coin_age(&self, current_ts: u64) -> f64 {
        let diff = current_ts.saturating_sub(self.genesis_ts);
        let days = diff / SECONDS_PER_DAY;
        days.max(1) as f64 // 确保非零，避免数学错误
    }

    /// 指数增长估值
    /// Math: age^slope * 10^intercept
    #[inline]
    fn calculate_exponential_growth(&self, coin_age_days: f64) -> f64 {
        coin_age_days.powf(self.slope) * self.intercept_factor
    }

    /// 计算预期价格（指数增长估值）
    pub fn expected_price(&self, timestamp: u64) -> f64 {
        let coin_age = self.calculate_coin_age(timestamp);
        self.calculate_exponential_growth(coin_age)
    }
}

impl Indicator for AHR {
    type Input = (f64, u64);
    type Output = Option<f64>;

    /// Input: (price, timestamp_seconds)
    fn on_data(&mut self, input: Self::Input) -> Self::Output {
        let (price, timestamp) = input;

        // 1. 更新 MA200 (若数据不足直接返回)
        let ma200 = self.ma200.on_data(price)?;

        // 2. 计算估值模型
        let coin_age = self.calculate_coin_age(timestamp);
        let expected_growth = self.calculate_exponential_growth(coin_age);

        // 3. 计算指标: P^2 / (MA * Expected)
        let ahr = (price * price) / (ma200 * expected_growth);

        Some(ahr)
    }
}

#[test]
fn test_ahr() {
    // Genesis = 0
    // Slope (a) = 2.0
    // Intercept (b) = -5.0 (即 factor = 10^-5 = 0.00001)
    let genesis_ts = 0;
    let slope = 2.0;
    let intercept = -5.0;

    let mut ahr = AHR::new(genesis_ts, slope, intercept);

    // 价格恒定为 100.0
    let constant_price = 100.0;

    // 我们需要喂入 200 个数据点才能触发计算
    // 第 200 天
    let day_200 = 200;
    let ts_200 = day_200 * 86400;

    // 1. 预热前 199 个点
    for i in 1..200 {
        let ts = i * 86400;
        ahr.on_data((constant_price, ts));
    }

    // 2. 喂入第 200 个点，触发计算
    let result = ahr.on_data((constant_price, ts_200));

    // 3. AHR
    // Formula: (P^2) / (MA * Expected)
    //        = (100^2) / (100 * 0.4)
    //        = 10000 / 40
    //        = 250.0
    let expected_ahr = 250.0;

    assert!(result.is_some(), "第 200 个点应该有返回值");
    let actual_ahr = result.unwrap();

    approx::assert_abs_diff_eq!(actual_ahr, expected_ahr);
}
