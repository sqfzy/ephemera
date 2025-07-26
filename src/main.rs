use af_xdp_ws::client::{
    IntoDataStream,
    okx::model::{OkxArg, OkxTradeData, OkxWsRequest},
};
use futures::StreamExt;

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
    let mut stream = req.into_stream().await.unwrap();

    while let Some(data) = stream.next().await {
        println!("debug0: {data:?}");
    }
}
