pub mod ahr;
pub mod bollinger;
pub mod ema;
pub mod iter;
pub mod ma;
pub mod mvrv;
pub mod rsi;
pub mod stream;
pub mod pi_cycle;

pub use ahr::*;
pub use bollinger::*;
pub use ema::*;
pub use iter::*;
pub use ma::*;
pub use mvrv::*;
pub use rsi::*;
pub use stream::*;
pub use pi_cycle::*;

pub trait Indicator {
    type Input;
    type Output;

    fn on_data(&mut self, input: Self::Input) -> Self::Output;
}

