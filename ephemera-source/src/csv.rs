use async_stream::stream;
use bytestring::ByteString;
use ephemera_shared::*;
use eyre::{Context, Result};
use futures::{Stream, StreamExt};
use serde::Deserialize;
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

        let mut records = reader.deserialize::<CsvTradeRecord>();

        while let Some(record) = records.next().await {
            match record {
                Ok(rec) => yield rec.try_into(),
                Err(e) => yield Err(e.into()),
            }
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

        let mut records = reader.deserialize::<CsvCandleRecord>();

        while let Some(record) = records.next().await {
            match record {
                Ok(rec) => yield rec.try_into(),
                Err(e) => yield Err(e.into()),
            }
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

        let mut records = reader.deserialize::<CsvBookRecord>();

        while let Some(record) = records.next().await {
            match record {
                Ok(rec) => yield rec.try_into(),
                Err(e) => yield Err(e.into()),
            }
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

        let mut records = reader.deserialize::<CsvTradeRecord>();
        let mut last_timestamp: Option<TimestampMs> = None;

        while let Some(record) = records.next().await {
            match record {
                Ok(rec) => {
                    let trade: TradeData = match rec.try_into() {
                        Ok(t) => t,
                        Err(e) => {
                            yield Err(e);
                            continue;
                        }
                    };

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

// ========== CSV 记录结构 ==========

#[derive(Debug, Deserialize)]
struct CsvTradeRecord {
    timestamp_ms: TimestampMs,
    symbol: String,
    price: f64,
    quantity: f64,
    side: String,
}

impl TryFrom<CsvTradeRecord> for TradeData {
    type Error = eyre::Error;

    fn try_from(rec: CsvTradeRecord) -> Result<Self> {
        let side = match rec.side.to_lowercase().as_str() {
            "buy" | "bid" => Side::Buy,
            "sell" | "ask" => Side::Sell,
            _ => eyre::bail!("Invalid side: {}", rec.side),
        };

        Ok(TradeData {
            symbol: ByteString::from(rec.symbol),
            price: rec.price,
            quantity: rec.quantity,
            side,
            timestamp_ms: rec.timestamp_ms,
        })
    }
}

#[derive(Debug, Deserialize)]
struct CsvCandleRecord {
    open_timestamp_ms: TimestampMs,
    symbol: String,
    interval_sc: u64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

impl TryFrom<CsvCandleRecord> for CandleData {
    type Error = eyre::Error;

    fn try_from(rec: CsvCandleRecord) -> Result<Self> {
        Ok(CandleData {
            symbol: ByteString::from(rec.symbol),
            interval_sc: rec.interval_sc,
            open_timestamp_ms: rec.open_timestamp_ms,
            open: rec.open,
            high: rec.high,
            low: rec.low,
            close: rec.close,
            volume: rec.volume,
        })
    }
}

#[derive(Debug, Deserialize)]
struct CsvBookRecord {
    timestamp: TimestampMs,
    symbol: String,
    bids: String, // "price1:size1;price2:size2"
    asks: String,
}

impl TryFrom<CsvBookRecord> for BookData {
    type Error = eyre::Error;

    fn try_from(rec: CsvBookRecord) -> Result<Self> {
        let parse_levels = |s: &str| -> Result<Vec<(f64, f64)>> {
            s.split(';')
                .filter(|s| !s.trim().is_empty())
                .map(|pair| {
                    let parts: Vec<&str> = pair.split(':').collect();
                    if parts.len() != 2 {
                        eyre::bail!("Invalid level pair: {}", pair);
                    }
                    Ok((parts[0].parse()?, parts[1].parse()?))
                })
                .collect()
        };

        Ok(BookData {
            symbol: ByteString::from(rec.symbol),
            timestamp: rec.timestamp,
            bids: parse_levels(&rec.bids)?,
            asks: parse_levels(&rec.asks)?,
        })
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
        writeln!(
            file,
            "timestamp_ms,symbol,price,quantity,side\n\
             1640000000000,BTC-USDT,50000.5,0.1,buy\n\
             1640000001000,ETH-USDT,4000.0,1.0,sell"
        )
        .unwrap();

        let mut stream = csv_trade_data_stream(file.path()).await.unwrap();

        let trade1 = stream.next().await.unwrap().unwrap();
        assert_eq!(trade1.symbol, "BTC-USDT");
        assert_eq!(trade1.price, 50000.5);
        assert_eq!(trade1.side, Side::Buy);

        let trade2 = stream.next().await.unwrap().unwrap();
        assert_eq!(trade2.symbol, "ETH-USDT");
        assert_eq!(trade2.side, Side::Sell);
    }

    #[tokio::test]
    async fn test_csv_candle_data_stream() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            "open_timestamp_ms,symbol,interval_sc,open,high,low,close,volume\n\
             1640000000000,BTC-USDT,60,50000.0,50100.0,49900.0,50050.0,10.5"
        )
        .unwrap();

        let mut stream = csv_candle_data_stream(file.path()).await.unwrap();

        let candle = stream.next().await.unwrap().unwrap();
        assert_eq!(candle.symbol, "BTC-USDT");
        assert_eq!(candle.open, 50000.0);
        assert_eq!(candle.volume, 10.5);
    }

    #[tokio::test]
    async fn test_csv_book_data_stream() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            "timestamp,symbol,bids,asks\n\
             1640000000000,BTC-USDT,50000:1.0;49999:2.0,50001:1.5;50002:3.0"
        )
        .unwrap();

        let mut stream = csv_book_data_stream(file.path()).await.unwrap();

        let book = stream.next().await.unwrap().unwrap();
        assert_eq!(book.symbol, "BTC-USDT");
        assert_eq!(book.bids.len(), 2);
        assert_eq!(book.asks.len(), 2);
        assert_eq!(book.bids[0], (50000.0, 1.0));
    }
}
