use std::str::FromStr;

use af_xdp_ws::{
    client::{
        okx::model::{OkxArg, OkxTradeData, OkxWsRequest},
        okx_xdp::model::XdpOkxWsRequest,
    },
    config::AppConfig,
    stream::IntoDataStream,
};
use log::LevelFilter;

#[tokio::main]
async fn main() {
    let config = AppConfig::load().expect("Failed to load config");

    env_logger::builder()
        .filter_level(LevelFilter::from_str(&config.log_level).unwrap())
        .init();

    let req = OkxWsRequest::<OkxTradeData> {
        end_point: "wss://ws.okx.com:433/ws/v5/public".into(),
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
