pub mod ahr;
pub mod bollinger;
pub mod ema;
pub mod iter;
pub mod ma;
pub mod mvrv;
pub mod rsi;
pub mod stream;

pub use ahr::*;
pub use bollinger::*;
pub use ema::*;
pub use iter::*;
pub use ma::*;
pub use mvrv::*;
pub use rsi::*;
pub use stream::*;

pub trait Indicator {
    type Input;
    type Output;

    fn next_value(&mut self, input: Self::Input) -> Self::Output;
}

