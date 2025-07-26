pub mod okx;

pub trait Request {
    type Response;
}

pub trait IntoDataStream {
    type Error;
    type Stream;

    fn into_stream(
        self,
    ) -> impl std::future::Future<Output = Result<Self::Stream, Self::Error>> + Send;
}

pub trait RawData {
    type Error;
    type Data;

    fn into_data(self) -> Result<Self::Data, Self::Error>;
}
