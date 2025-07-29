pub mod okx;
pub mod okx_xdp;

pub trait Request {
    type Response;
}

pub trait RawData {
    type Error;
    type Data;

    fn into_data(self) -> Result<Self::Data, Self::Error>;
}
