#![allow(dead_code)]

pub mod model;

// // TODO: 新struct，用phantom标记OkxData
// type WsClient = StringCodec<TlsStream<AfXdpStream>>;
//
// pub struct OkxClientV5 {
//     base_http_url: Url,
//     base_ws_uri: Uri,
//
//     // TODO: 存储多个ws_client，对应不同频道
//     ws_client: WsClient,
//     http_client: Client,
//     api_key: Option<ByteString>,        // 即 OK_ACCESS_KEY
//     api_passphrase: Option<ByteString>, // 即 OK_ACCESS_PASSPHRASE
//
//     itoa_buffer: itoa::Buffer, // 高效的整数转为字符串的缓冲区
//     bytes_buffer: Vec<u8>,
// }
//
// #[bon::bon]
// impl OkxClientV5 {
//     #[builder]
//     pub fn new(
//         /// OK_ACCESS_KEY
//         api_key: Option<ByteString>,
//         /// OK_ACCESS_PASSPHRASE
//         api_passphrase: Option<ByteString>,
//
//         xsk_if_name: ByteString,
//         iface: Interface,
//     ) -> Result<Self> {
//         let ok_access_key = std::env::var("OK_ACCESS_KEY")
//             .ok()
//             .map(ByteString::from)
//             .or(api_key);
//
//         let ok_access_passphrase = std::env::var("OK_ACCESS_PASSPHRASE")
//             .ok()
//             .map(ByteString::from)
//             .or(api_passphrase);
//
//         let base_http_url = "https://www.okx.com/".parse::<Url>()?;
//         let base_ws_uri = "wss://wspap.okx.com/".parse::<Uri>()?;
//
//         let device = XskDevice::new(&xsk_if_name)?;
//         let stream = AfXdpStream::connect(
//             base_ws_uri.to_string().parse::<SocketAddr>()?,
//             iface,
//             device,
//         )?;
//         let tls_stream = wrap_native_tls(stream, get_host(&base_ws_uri)?, vec![])?;
//         let ws_client = ws_tool::ClientBuilder::new().with_stream(
//             base_ws_uri.clone(),
//             tls_stream,
//             ws_tool::codec::StringCodec::check_fn,
//         )?;
//
//         Ok(OkxClientV5 {
//             base_http_url,
//             base_ws_uri,
//             ws_client,
//             http_client: Client::new(),
//             api_key: ok_access_key,
//             api_passphrase: ok_access_passphrase,
//             itoa_buffer: itoa::Buffer::new(),
//             bytes_buffer: Vec::new(),
//         })
//     }
//
//     fn with_buffer<F, R>(&mut self, f: F) -> R
//     where
//         F: FnOnce(&mut Vec<u8>) -> R,
//     {
//         self.bytes_buffer.clear();
//
//         f(&mut self.bytes_buffer)
//     }
//
//     // fn buffer_json() {}
// }
//
// impl Exchange for OkxClientV5 {
//     fn subscribe_book_block(
//         &self,
//         symbol: &str,
//         interval_ms: u64,
//         max_level: u64,
//     ) -> Result<impl Iterator<Item = Result<BookData>>> {
//         let op = "subscribe";
//         let arg = match (interval_ms, max_level) {
//             (100, u64::MAX) => OkxArg {
//                 channel: "books".into(),
//                 inst_id: symbol.into(),
//             },
//             (100, 5) => OkxArg {
//                 channel: "books5".into(),
//                 inst_id: symbol.into(),
//             },
//             (10, 1) => OkxArg {
//                 channel: "bbo-tbt".into(),
//                 inst_id: symbol.into(),
//             },
//             (_, _) => bail!(
//                 "Unsupported interval_ms: {}, max_level: {}",
//                 interval_ms,
//                 max_level
//             ),
//         };
//         let args = vec![arg];
//
//         let request = OkxWsRequest {
//             op: op.into(),
//             args,
//             ..Default::default()
//         };
//         self.ws_client
//             .send(simd_json::to_string(&request)?.as_str())?;
//
//         self.ws_client.receive_raw()?;
//
//         Ok(self.ws_client)
//     }
// }
//
// pub struct OkxDataStream<S, R> {
//     stream: S,
//     _phantom: std::marker::PhantomData<R>,
// }
//
// impl<S, R> OkxDataStream<S, R> {
//     pub fn new(s: S) -> Self {
//         Self {
//             stream: s,
//             buffer: vec![],
//             _phantom: std::marker::PhantomData,
//         }
//     }
// }
//
// impl Iterator for OkxDataStream<&'_ mut WsClient, Vec<BookData>> {
//     type Item = Result<Vec<BookData>>;
//
//     fn next(&mut self) -> Option<Self::Item> {
//         Zip
//         Some(
//             try {
//                 loop {
//                     let data = self.stream.receive_raw()?.data;
//                     let resp = simd_json::from_slice::<OkxWsDataResponse<OkxBookData>>(
//                         &mut data.to_vec(),
//                     )?;
//                     break resp
//                         .data
//                         .into_iter()
//                         .map(BookData::try_from)
//                         .collect::<Result<Vec<_>>>()?;
//                 }
//             },
//         )
//     }
// }
