pub mod binance;
pub mod okx;
pub mod utils;
// pub mod okx_xdp;

use bytestring::ByteString;
use futures::Stream;

use crate::Timestamp;

// pub trait Request {
//     type Response;
// }
//
// pub trait RawData {
//     type Error;
//     type Data;
//
//     fn into_data(self) -> Result<Self::Data, Self::Error>;
// }
//
// pub trait DataSource {
//     type Error;
//     type Stream;
//
//     fn data_stream(self) -> impl Future<Output = Result<Self::Stream, Self::Error>>;
// }

// async fn recent_trade_data_stream(
//     symbol_and_channel: Vec<(ByteString, ByteString)>,
// ) -> eyre::Result<impl Stream> {
//     eyre::bail!("Dont support!")
// }
//
// /// 获取历史K线数据
// ///
// /// # Arguments
// /// * `symbol`: 交易对，例如 "BTCUSDT"
// /// * `interval`: K线的时间间隔，例如 1分钟、1小时、1天
// /// * `start_time`: 数据开始时间 (包含)
// /// * `end_time`: 数据结束时间 (不包含)
// async fn historical_candle_data(
//     symbol: ByteString,
//     interval: Duration,
//     start_time: Timestamp,
//     end_time: Timestamp,
// ) -> eyre::Result<Vec<Kline>> {
//     eyre::bail!("Dont support!")
// }
