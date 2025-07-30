use eyre::{ContextCompat, ensure};
use serde::de::DeserializeOwned;
use std::ops::{Deref, DerefMut};
use ws_tool::{
    connector::{get_host, wrap_native_tls},
    frame::OpCode,
};

use crate::{
    af_xdp::stream::XdpStream,
    client::{
        RawData,
        okx::model::{OkxWsDataResponse, OkxWsRequest, OkxWsResponse},
    },
    stream::IntoDataStream,
};

#[derive(Default)]
pub struct XdpOkxWsRequest<D>(pub OkxWsRequest<D>);

impl<D> Deref for XdpOkxWsRequest<D> {
    type Target = OkxWsRequest<D>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<D> DerefMut for XdpOkxWsRequest<D> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<D> IntoDataStream for XdpOkxWsRequest<D>
where
    D: RawData<Error = eyre::Error> + DeserializeOwned + Send,
    D::Data: Send + 'static,
{
    type Error = eyre::Error;
    type Stream = Box<dyn Iterator<Item = Result<D::Data, Self::Error>>>;

    async fn into_stream(self) -> Result<Self::Stream, Self::Error> {
        let uri = self.end_point.parse::<http::Uri>()?;
        let host = uri.host().wrap_err("Invalid URI: missing host")?;
        // TODO: 
        // let host = "104.18.43.174";
        let port = uri.port_u16().wrap_err("Invalid URI: missing port")?;

        let stream = XdpStream::connect((host, port))?;
        let tls_stream = wrap_native_tls(stream, get_host(&uri)?, vec![])?;

        let mut client = ws_tool::ClientBuilder::new().with_stream(
            uri,
            tls_stream,
            ws_tool::codec::StringCodec::check_fn,
        )?;

        client.send(&simd_json::to_string(&self.0)?)?;

        let resp = simd_json::from_slice::<OkxWsResponse>(client.receive_raw()?.data.to_mut())?;
        ensure!(
            resp.event == "subscribe",
            "{}: {}",
            resp.code.unwrap_or_default(),
            resp.msg.unwrap_or_default()
        );

        let stream = gen move {
            loop {
                let mut msg = match client.receive_raw() {
                    Ok(msg) => msg,
                    Err(e) => {
                        yield Err(e.into());
                        continue;
                    }
                };

                if msg.code == OpCode::Close {
                    break;
                }

                match simd_json::from_slice::<OkxWsDataResponse<D>>(msg.data.to_mut()) {
                    Ok(resp) => {
                        for data in resp.data {
                            yield data.into_data()
                        }
                    }
                    Err(e) => yield Err(e.into()),
                }
            }
        };

        Ok(Box::new(stream))
    }
}
