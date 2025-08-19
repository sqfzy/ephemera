use ephemera::{data::TradeData, source::okx::*};
use futures::StreamExt;
use std::time::Duration;

// #[tokio::test]
// #[ignore]
// async fn test_ws_request() {
//     let mut stream = okx_subscribe_caller::<TradeData>(OKX_WS_PUBLICE_ENDPOINT)
//         .arg("trades", "BTC-USDT")
//         .arg("trades", "ETH-USDT")
//         .call()
//         .await
//         .unwrap();
//
//     for _ in 0..10 {
//         let trade_data = stream.next().await.unwrap().unwrap();
//
//         assert!(trade_data.symbol == "BTC-USDT" || trade_data.symbol == "ETH-USDT");
//         assert!(!trade_data.trade_id.is_empty());
//         assert!(trade_data.price > 0.0);
//         assert!(trade_data.quantity > 0.0);
//         assert!(trade_data.timestamp > 1_609_459_200_000); // 时间戳在 2021 年之后
//     }
// }
