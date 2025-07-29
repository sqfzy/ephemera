use af_xdp_ws::{
    client::{
        okx::model::{OkxArg, OkxTradeData, OkxWsRequest},
        okx_xdp::model::XdpOkxWsRequest,
    },
    stream::IntoDataStream,
};

#[tokio::main]
async fn main() {
    let req = OkxWsRequest::<OkxTradeData> {
        end_point: "wss://ws.okx.com/ws/v5/public".into(),
        op: "subscribe".into(),
        args: vec![OkxArg {
            channel: "trades".into(),
            inst_id: "BTC-USDT".into(),
        }],
        ..Default::default()
    };
    let xdpreq = XdpOkxWsRequest(req);
    let stream = xdpreq.into_stream().await.unwrap();

    for data in stream {
        println!("debug0: {data:?}");
    }
}
