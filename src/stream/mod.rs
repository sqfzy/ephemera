pub mod http;
pub mod websocket;

pub trait IntoDataStream {
    type Error;
    type Stream;

    fn into_stream(self) -> impl Future<Output = Result<Self::Stream, Self::Error>> + Send;
}
