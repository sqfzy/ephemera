#[tokio::main]
async fn main() {
    // let config = ephemera::config::EphemaraConfig::load().expect("Failed to load config");

    // let req = OkxWsRequest::<OkxTradeData> {
    //     end_point: "wss://ws.okx.com:433/ws/v5/public".into(),
    //     op: "subscribe".into(),
    //     args: vec![OkxArg {
    //         channel: "trades".into(),
    //         inst_id: "BTC-USDT".into(),
    //     }],
    //     ..Default::default()
    // };
    // let xdpreq = XdpOkxWsRequest(req);
    // let stream = xdpreq.into_stream().await.unwrap();
    //
    // for data in stream {
    //     println!("debug0: {data:?}");
    // }
}
