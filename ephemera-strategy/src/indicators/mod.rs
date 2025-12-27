pub mod ahr;
pub mod iter;
pub mod ma;
pub mod mvrv;
pub mod stream;

pub use ahr::*;
pub use iter::*;
pub use ma::*;
pub use mvrv::*;
pub use stream::*;

pub trait Indicator {
    type Input;
    type Output;

    fn next_value(&mut self, input: Self::Input) -> Self::Output;
}
