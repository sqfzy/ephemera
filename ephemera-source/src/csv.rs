use async_stream::stream;
use ephemera_shared::*;
use eyre::{Context, Result};
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::{path::Path, pin::Pin};
use tokio::{
    fs::File,
    time::{Duration, sleep},
};

/// CSV 交易数据流
///
/// CSV 格式：timestamp_ms,symbol,price,quantity,side
pub async fn csv_trade_data_stream(
    path: impl AsRef<Path>,
) -> Result<impl Stream<Item = Result<TradeData>>> {
    let path = path.as_ref().to_path_buf();
    let file = File::open(&path)
        .await
        .with_context(|| format!("Failed to open file: {}", path.display()))?;

    let stream = stream! {
        let mut reader = csv_async::AsyncReaderBuilder::new()
            .has_headers(true)
            .create_deserializer(file);

        let mut records = reader.deserialize::<TradeData>();

        while let Some(record) = records.next().await {
            yield record.map_err(Into::into)
        }
    };

    Ok(Box::pin(stream) as Pin<Box<dyn Stream<Item = Result<TradeData>> + Send>>)
}

/// CSV K线数据流
///
/// CSV 格式：open_timestamp_ms,symbol,interval_sc,open,high,low,close,volume
pub async fn csv_candle_data_stream(
    path: impl AsRef<Path>,
) -> Result<impl Stream<Item = Result<CandleData>>> {
    let path = path.as_ref().to_path_buf();
    let file = File::open(&path)
        .await
        .with_context(|| format!("Failed to open file: {}", path.display()))?;

    let stream = stream! {
        let mut reader = csv_async::AsyncReaderBuilder::new()
            .has_headers(true)
            .create_deserializer(file);

        let mut records = reader.deserialize::<CandleData>();

        while let Some(record) = records.next().await {
            yield record.map_err(Into::into)
        }
    };

    Ok(Box::pin(stream))
}

/// CSV 订单簿数据流
///
/// CSV 格式：timestamp,symbol,bids,asks
/// bids/asks 格式：price1:size1;price2:size2
pub async fn csv_book_data_stream(
    path: impl AsRef<Path>,
) -> Result<impl Stream<Item = Result<BookData>>> {
    let path = path.as_ref().to_path_buf();
    let file = File::open(&path)
        .await
        .with_context(|| format!("Failed to open file: {}", path.display()))?;

    let stream = stream! {
        let mut reader = csv_async::AsyncReaderBuilder::new()
            .has_headers(true)
            .create_deserializer(file);

        let mut records = reader.deserialize::<RawBookData>();

        while let Some(record) = records.next().await {
            yield record.map(Into::into).map_err(Into::into)
        }
    };

    Ok(Box::pin(stream))
}

/// 带时间模拟的交易数据流（按时间戳回放）
pub async fn csv_trade_data_stream_with_replay(
    path: impl AsRef<Path>,
    speed: f64, // 播放速度倍数，1.0 为实时，2.0 为 2x 速度
) -> Result<impl Stream<Item = Result<TradeData>>> {
    let path = path.as_ref().to_path_buf();
    let file = File::open(&path)
        .await
        .with_context(|| format!("Failed to open file: {}", path.display()))?;

    let stream = stream! {
        let mut reader = csv_async::AsyncReaderBuilder::new()
            .has_headers(true)
            .create_deserializer(file);

        let mut records = reader.deserialize::<TradeData>();
        let mut last_timestamp: Option<TimestampMs> = None;

        while let Some(record) = records.next().await {
            match record {
                Ok(trade) => {
                    // 模拟时间延迟
                    if let Some(last_ts) = last_timestamp {
                        let delay_ms = trade.timestamp_ms.saturating_sub(last_ts);
                        if delay_ms > 0 {
                            let delay = Duration::from_millis((delay_ms as f64 / speed) as u64);
                            sleep(delay).await;
                        }
                    }
                    last_timestamp = Some(trade.timestamp_ms);
                    yield Ok(trade);
                }
                Err(e) => yield Err(e.into()),
            }
        }
    };

    Ok(Box::pin(stream))
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RawBookData {
    pub symbol: Symbol,
    pub timestamp: TimestampMs,
    /// (价格, 数量)
    #[serde(with = "json_string")]
    pub bids: BookSide,
    /// (价格, 数量)
    #[serde(with = "json_string")]
    pub asks: BookSide,
}

mod json_string {
    use super::*;
    use serde::{Deserializer, Serializer, de::Error as DeError};

    pub fn serialize<S>(data: &BookSide, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let json_str = simd_json::to_string(data).map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&json_str)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<BookSide, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut s = String::deserialize(deserializer)?.into_bytes();
        simd_json::from_slice(&mut s).map_err(D::Error::custom)
    }
}

impl From<RawBookData> for BookData {
    fn from(value: RawBookData) -> Self {
        Self {
            symbol: value.symbol,
            timestamp: value.timestamp,
            bids: value.bids,
            asks: value.asks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_csv_trade_data_stream() {
        let mut file = NamedTempFile::new().unwrap();

        file.write_all(
            [
                r#"timestamp_ms,symbol,price,quantity,side"#,
                r#"1640000000000,BTC-USDT,50000.5,0.1,Buy"#,
                r#"1640000001000,ETH-USDT,4000.0,1.0,Sell"#,
            ]
            .join("\n")
            .as_bytes(),
        )
        .unwrap();

        let mut stream = csv_trade_data_stream(file.path()).await.unwrap();

        let trade1 = stream.next().await.unwrap().unwrap();
        assert_eq!(trade1.symbol, "BTC-USDT");
        assert_eq!(trade1.price, 50000.5);
        assert_eq!(trade1.quantity, 0.1);
        assert_eq!(trade1.side, Side::Buy);
        assert_eq!(trade1.timestamp_ms, 1640000000000);

        let trade2 = stream.next().await.unwrap().unwrap();
        assert_eq!(trade2.symbol, "ETH-USDT");
        assert_eq!(trade2.price, 4000.0);
        assert_eq!(trade2.side, Side::Sell);
    }

    #[tokio::test]
    async fn test_csv_candle_data_stream() {
        let mut file = NamedTempFile::new().unwrap();

        file.write_all(
            [
                r#"symbol,interval_sc,open_timestamp_ms,open,high,low,close,volume"#,
                r#"BTC-USDT,60,1640000000000,50000.0,50100.0,49900.0,50050.0,10.5"#,
                r#"ETH-USDT,60,1640000060000,4000.0,4010.0,3990.0,4005.0,100.0"#,
            ]
            .join("\n")
            .as_bytes(),
        )
        .unwrap();

        let mut stream = csv_candle_data_stream(file.path()).await.unwrap();

        let candle1 = stream.next().await.unwrap().unwrap();
        assert_eq!(candle1.symbol, "BTC-USDT");
        assert_eq!(candle1.interval_sc, 60);
        assert_eq!(candle1.open_timestamp_ms, 1640000000000);
        assert_eq!(candle1.open, 50000.0);
        assert_eq!(candle1.high, 50100.0);
        assert_eq!(candle1.low, 49900.0);
        assert_eq!(candle1.close, 50050.0);
        assert_eq!(candle1.volume, 10.5);

        let candle2 = stream.next().await.unwrap().unwrap();
        assert_eq!(candle2.symbol, "ETH-USDT");
    }

    #[tokio::test]
    async fn test_csv_book_data_stream() {
        let mut file = NamedTempFile::new().unwrap();

        // 对于复杂的 JSON 嵌套 CSV，r#""# 是最佳选择
        file.write_all(
            [
                r#"symbol,timestamp,bids,asks"#,
                r#"BTC-USDT,1640000000000,"[[50000.0, 1.0], [49999.0, 2.0]]","[[50001.0, 1.5], [50002.0, 3.0]]""#,
                r#"ETH-USDT,1640000001000,"[[4000.0, 10.0]]","[[4001.0, 15.0]]""#,
            ]
            .join("\n")
            .as_bytes(),
        )
        .unwrap();

        let mut stream = csv_book_data_stream(file.path()).await.unwrap();

        let book1 = stream.next().await.unwrap().unwrap();
        assert_eq!(book1.symbol, "BTC-USDT");
        assert_eq!(book1.timestamp, 1640000000000);
        assert_eq!(book1.bids.len(), 2);
        assert_eq!(book1.asks.len(), 2);
        assert_eq!(book1.bids[0], (50000.0, 1.0));
        assert_eq!(book1.bids[1], (49999.0, 2.0));
        assert_eq!(book1.asks[0], (50001.0, 1.5));
        assert_eq!(book1.asks[1], (50002.0, 3.0));

        let book2 = stream.next().await.unwrap().unwrap();
        assert_eq!(book2.symbol, "ETH-USDT");
        assert_eq!(book2.bids.len(), 1);
    }

    #[tokio::test]
    async fn test_csv_trade_data_stream_with_replay() {
        let mut file = NamedTempFile::new().unwrap();

        file.write_all(
            [
                r#"timestamp_ms,symbol,price,quantity,side"#,
                r#"1640000000000,BTC-USDT,50000.0,0.1,Buy"#,
                r#"1640000001000,BTC-USDT,50001.0,0.2,Sell"#,
            ]
            .join("\n")
            .as_bytes(),
        )
        .unwrap();

        let start = tokio::time::Instant::now();
        let mut stream = csv_trade_data_stream_with_replay(file.path(), 10.0)
            .await
            .unwrap();

        let _trade1 = stream.next().await.unwrap().unwrap();
        let _trade2 = stream.next().await.unwrap().unwrap();
        let elapsed = start.elapsed();

        // 原本 1000ms 延迟，10x 速度应该约为 100ms
        assert!(elapsed.as_millis() >= 80 && elapsed.as_millis() <= 200);
    }

    #[tokio::test]
    async fn test_empty_csv() {
        let mut file = NamedTempFile::new().unwrap();

        // 即使只有一行，也可以保持风格一致，或者直接用 write_all
        file.write_all(r#"timestamp_ms,symbol,price,quantity,side"#.as_bytes())
            .unwrap();

        let mut stream = csv_trade_data_stream(file.path()).await.unwrap();
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn test_invalid_csv_format() {
        let mut file = NamedTempFile::new().unwrap();

        file.write_all(
            [
                r#"timestamp_ms,symbol,price,quantity,side"#,
                r#"invalid,data,here"#,
            ]
            .join("\n")
            .as_bytes(),
        )
        .unwrap();

        let mut stream = csv_trade_data_stream(file.path()).await.unwrap();
        let result = stream.next().await.unwrap();
        assert!(result.is_err());
    }
}
