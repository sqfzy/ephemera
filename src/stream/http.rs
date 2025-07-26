use crate::client::Request;
use futures::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
struct HttpStream<R, D> {
    client: Client,
    req: R,
    phantom: std::marker::PhantomData<D>,
}

// impl<R, D> HttpStream<R, D> {
//     pub fn new(client: Client, req: R) -> Self {
//         Self {
//             client,
//             req,
//             phantom: std::marker::PhantomData,
//         }
//     }
// }
//
// impl<R: Serialize + Request, D> Stream for HttpStream<R, D>
// where
//     R::Response: TryInto<D> + Deserialize,
// {
//     type Item = Result<D, eyre::Report>;
//
//     fn poll_next(
//         self: std::pin::Pin<&mut Self>,
//         cx: &mut std::task::Context<'_>,
//     ) -> std::task::Poll<Option<Self::Item>> {
//         let this = self.get_mut();
//         match this.client.post(this.req).send() {
//             Ok(response) => match response.json::<R::Response>() {
//                 Ok(data) => {
//                     let item: D = data.try_into().map_err(|e| eyre::Report::from(e))?;
//                     std::task::Poll::Ready(Some(Ok(item)))
//                 }
//                 Err(e) => std::task::Poll::Ready(Some(Err(eyre::Report::from(e)))),
//             },
//             Err(e) => std::task::Poll::Ready(Some(Err(eyre::Report::from(e)))),
//         }
//     }
// }
