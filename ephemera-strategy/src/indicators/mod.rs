pub mod bollinger;
pub mod ema;
pub mod ma;
pub mod rsi;
pub mod sar;

pub use bollinger::*;
pub use ema::*;
pub use ma::*;
pub use rsi::*;
pub use sar::*;


pub trait Indicator: Send + Sync {
    /// 输入数据类型
    type Input;
    /// 输出值类型
    type Output;

    /// 更新指标值
    fn update(&mut self, input: Self::Input) -> Option<Self::Output>;

    /// 获取当前指标值
    fn value(&self) -> Option<Self::Output>;
}
