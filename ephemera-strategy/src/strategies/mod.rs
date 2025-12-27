pub trait Strategy {
    type Input;
    type Error;

    fn process(&mut self, input: Self::Input) -> Result<ephemera_shared::Signal, Self::Error>;
}
