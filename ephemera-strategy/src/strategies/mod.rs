// pub mod harvest;
// pub mod rsi;
// pub mod smart_dca;

// pub use harvest::*;
// pub use rsi::*;
// pub use smart_dca::*;

use ephemera_shared::Signal;

pub trait Strategy {
    type Input;
    type Error;

    fn process(&mut self, input: Self::Input) -> Result<Signal, Self::Error>;
}
