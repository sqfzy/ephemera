use crate::error::DataError;
use crate::*;
use futures::stream::Peekable;
use futures::{Stream, StreamExt};
use std::pin::Pin;

/// Transforms a stream of into a stream of candles.
///
/// # Panics
///
/// See [`agg_trades_to_candle`]
///
/// # Examples
///
/// ```rust
/// # use futures::{stream, StreamExt};
/// # use rust_decimal_macros::dec;
/// # use my_crate::{trades_to_candle_stream, TradeData, Side, CandleData};
/// #
/// # #[tokio::main]
/// # async fn main() {
/// let trades: Vec<TradeData> = vec![
///     // Candle #1 (10:00:00 -> 10:01:00)
///     TradeData { symbol: "BTC-USDT".into(), timestamp_ms: 1756202405000, price: dec!(20000), quantity: dec!(1.5), side: Side::Buy },
///     TradeData { symbol: "BTC-USDT".into(), timestamp_ms: 1756202455000, price: dec!(20100), quantity: dec!(2.0), side: Side::Buy },
///     // Candle #2 (10:02:00 -> 10:03:00)
///     TradeData { symbol: "BTC-USDT".into(), timestamp_ms: 1756202525000, price: dec!(20120), quantity: dec!(3.0), side: Side::Buy },
/// ];
///
/// let trade_stream = stream::iter(trades);
///
/// let mut candle_stream = trades_to_candle_stream(trade_stream, 60);
///
/// let candle1 = candle_stream.next().await.unwrap();
/// assert_eq!(candle1.open_timestamp, 1756202400000);
/// assert_eq!(candle1.high, dec!(20100));
/// assert_eq!(candle1.volume, dec!(3.5));
///
/// let candle2 = candle_stream.next().await.unwrap();
/// assert_eq!(candle2.open_timestamp, 1756202520000);
/// assert_eq!(candle2.volume, dec!(3.0));
///
/// assert!(candle_stream.next().await.is_none());
/// # }
/// ```
pub fn transform_trades_to_candles(
    stream: impl Stream<Item = TradeData> + Unpin + Send,
    interval_sc: u64,
) -> impl Stream<Item = DataResult<CandleData>> + Send {
    let stream = stream.peekable();
    futures::stream::unfold(Box::pin(stream), move |mut s| async move {
        agg_trades_to_candle(s.as_mut(), interval_sc)
            .await
            .transpose()
            .map(|candle| (candle, s))
    })
}

/// A low-level helper to aggregate trades from a stream into a single candle.
/// **Assume the trade data at the end of the line constitutes a complete candle.**
///
/// # Error
///
/// See ['CandleData::agg_with_trade'].
///
/// # Panics
///
/// 1. If `target_interval` is `0`.
pub async fn agg_trades_to_candle(
    mut stream: Pin<&mut Peekable<impl Stream<Item = TradeData> + Unpin>>,
    target_interval: IntervalSc,
) -> DataResult<Option<CandleData>> {
    assert_ne!(target_interval, 0, "Interval shouldn't be zero.");
    let interval_ms = target_interval * 1000;

    let Some(first_trade) = stream.next().await else {
        return Ok(None);
    };

    let mut candle = CandleData::new_with_trade(&first_trade, target_interval);
    let close_timestamp = candle.open_timestamp_ms + interval_ms;

    while let Some(next_trade) = stream
        .as_mut()
        .next_if(|t| t.timestamp_ms < close_timestamp)
        .await
    {
        candle.agg_with_trade(&next_trade)?;
    }

    Ok(Some(candle))
}

/// Aggregates a stream of smaller-interval candles into a stream of larger-interval candles.
/// *Incomplete groups at the end of the stream are discarded*.
///
/// # Error
///
/// See ['agg_candles_to_candle'].
///
/// # Examples
/// ```rust
/// # use futures::{stream, StreamExt};
/// # use rust_decimal_macros::dec;
/// # use my_crate::{candles_to_candle_stream, CandleData};
/// #
/// # #[tokio::main]
/// # async fn main() {
/// let minute_candles: Vec<CandleData> = vec![
///     // Group 1
///     CandleData { symbol: "BTC-USDT".into(), interval_sc: 60, open_timestamp: 1672531200000, open: dec!(20000), high: dec!(20100), low: dec!(19950), close: dec!(20050), volume: dec!(10) },
///     CandleData { symbol: "BTC-USDT".into(), interval_sc: 60, open_timestamp: 1672531260000, open: dec!(20050), high: dec!(20200), low: dec!(20040), close: dec!(20180), volume: dec!(15) },
///     // Incomplete group at the end
///     CandleData { symbol: "BTC-USDT".into(), interval_sc: 60, open_timestamp: 1672531320000, open: dec!(20180), high: dec!(20190), low: dec!(20150), close: dec!(20160), volume: dec!(12) },
/// ];
///
/// let candle_stream = stream::iter(minute_candles);
///
/// // Aggregate 2x 1-minute candles into 2-minute candles
/// let mut two_minute_stream = candles_to_candle_stream(candle_stream, 120);
///
/// let candle1 = two_minute_stream.next().await.unwrap();
/// assert_eq!(candle1.interval_sc, 120);
/// assert_eq!(candle1.open, dec!(20000));
/// assert_eq!(candle1.high, dec!(20200));
/// assert_eq!(candle1.close, dec!(20180));
/// assert_eq!(candle1.volume, dec!(25)); // 10 + 15
///
/// // The stream ends because the last candle forms an incomplete group
/// assert!(two_minute_stream.next().await.is_none());
/// # }
/// ```
pub fn transform_candles_to_candles(
    candle_stream: impl Stream<Item = CandleData> + Unpin + Send,
    target_interval: IntervalSc,
) -> impl Stream<Item = DataResult<CandleData>> + Send {
    futures::stream::unfold(candle_stream, move |mut stream| async move {
        agg_candles_to_candle(&mut stream, target_interval)
            .await
            .transpose()
            .map(|candle| (candle, stream))
    })
}

/// A low-level helper to aggregate a fixed number of candles, from a stream into a single candle
/// which's interval is larger.
///
/// # Error
///
/// 1. If target_interval is not multiple of first interval_sc.
/// 2. If symbol or interval_sc mismatched.
/// 3. If timestamp order is violated.
///
/// # Panics
///
/// 1. If `target_interval` is `0`.
pub async fn agg_candles_to_candle(
    stream: &mut (impl Stream<Item = CandleData> + Unpin),
    target_interval: IntervalSc,
) -> DataResult<Option<CandleData>> {
    assert_ne!(target_interval, 0, "Interval shouldn't be zero.");

    let Some(first_candle) = stream.next().await else {
        return Ok(None);
    };

    let first_interval = first_candle.interval_sc;

    if !target_interval.is_multiple_of(first_interval) {
        return Err(DataError::UnDivisibleInterval {
            target: target_interval,
            base: first_interval,
        });
    }

    let n = target_interval / first_interval;
    let mut agg_candle = first_candle;

    for _ in 0..(n - 1) {
        let Some(next_candle) = stream.next().await else {
            break;
        };

        if next_candle.symbol != agg_candle.symbol {
            return Err(DataError::MismatchedSymbol {
                expected: agg_candle.symbol.clone(),
                found: next_candle.symbol.clone(),
            });
        }

        if next_candle.interval_sc != first_interval {
            return Err(DataError::MismatchedInterval {
                expected: agg_candle.interval_sc,
                found: next_candle.interval_sc,
            });
        }

        if next_candle.open_timestamp_ms <= agg_candle.open_timestamp_ms {
            return Err(DataError::timestamp_should_be_after(
                agg_candle.open_timestamp_ms,
                next_candle.open_timestamp_ms,
            ));
        }

        agg_candle.unchecked_agg_with_candle(&next_candle);
    }

    Ok((agg_candle.interval_sc == target_interval).then_some(agg_candle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Side, TradeData};
    use futures::{StreamExt, TryStreamExt, stream};
    use rust_decimal::dec;

    /// 测试正常聚合：输入流包含足够完成一次聚合的交易，并且还有剩余。
    #[tokio::test]
    async fn test_agg_trades_to_candle_with_remainder() {
        let trades: Vec<TradeData> = vec![
            // 这些属于第一个60s K线 (10:00:00 -> 10:01:00)
            TradeData {
                symbol: "BTC-USDT".into(),
                timestamp_ms: 1756202405000,
                price: dec!(100),
                quantity: dec!(1.0),
                side: Side::Buy,
            },
            TradeData {
                symbol: "BTC-USDT".into(),
                timestamp_ms: 1756202420000,
                price: dec!(120),
                quantity: dec!(2.0),
                side: Side::Sell,
            },
            TradeData {
                symbol: "BTC-USDT".into(),
                timestamp_ms: 1756202450000,
                price: dec!(80),
                quantity: dec!(1.5),
                side: Side::Buy,
            },
            // 这个属于下一个K线，不应该被消耗
            TradeData {
                symbol: "BTC-USDT".into(),
                timestamp_ms: 1756202465000,
                price: dec!(150),
                quantity: dec!(3.0),
                side: Side::Buy,
            },
        ];

        let mut stream = Box::pin(stream::iter(trades).peekable());
        let candle = agg_trades_to_candle(stream.as_mut(), 60)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(candle.open, dec!(100));
        assert_eq!(candle.high, dec!(120));
        assert_eq!(candle.low, dec!(80));
        assert_eq!(candle.close, dec!(80));
        assert_eq!(candle.volume, dec!(4.5));
        assert_eq!(candle.open_timestamp_ms, 1756202400000);

        // 断言流中还剩下未被消耗的数据
        let remaining_trade = stream.next().await.unwrap();
        assert_eq!(remaining_trade.price, dec!(150));
        assert!(stream.next().await.is_none(), "Stream should be empty now");
    }

    /// 测试流在聚合中途结束的场景，应能正确返回已聚合的部分。
    #[tokio::test]
    async fn test_agg_trades_to_candle_stream_end() {
        let trades: Vec<TradeData> = vec![
            TradeData {
                symbol: "BTC-USDT".into(),
                timestamp_ms: 1756202405000,
                price: dec!(200),
                quantity: dec!(1.0),
                side: Side::Buy,
            },
            TradeData {
                symbol: "BTC-USDT".into(),
                timestamp_ms: 1756202420000,
                price: dec!(210),
                quantity: dec!(2.0),
                side: Side::Sell,
            },
        ];

        let mut stream = Box::pin(stream::iter(trades).peekable());
        let candle = agg_trades_to_candle(stream.as_mut(), 60)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(candle.open, dec!(200));
        assert_eq!(candle.close, dec!(210));
        assert_eq!(candle.volume, dec!(3.0));
    }

    /// 测试输入流为空的场景，应返回 None。
    #[tokio::test]
    async fn test_agg_trades_to_candle_empty_stream() {
        let trades: Vec<TradeData> = vec![];
        let mut stream = Box::pin(stream::iter(trades).peekable());
        let result = agg_trades_to_candle(stream.as_mut(), 60).await.unwrap();
        assert!(result.is_none());
    }

    /// 测试正常聚合：输入流包含足够完成一次聚合的K线，并且还有剩余。
    #[tokio::test]
    async fn test_agg_candles_to_candle_with_remainder() {
        let all_candles: Vec<CandleData> = vec![
            CandleData {
                symbol: "BTC-USDT".into(),
                interval_sc: 60,
                open_timestamp_ms: 1672531200000,
                open: dec!(200),
                high: dec!(210),
                low: dec!(190),
                close: dec!(205),
                volume: dec!(10),
            },
            CandleData {
                symbol: "BTC-USDT".into(),
                interval_sc: 60,
                open_timestamp_ms: 1672531260000,
                open: dec!(205),
                high: dec!(220),
                low: dec!(202),
                close: dec!(218),
                volume: dec!(15),
            },
            CandleData {
                symbol: "BTC-USDT".into(),
                interval_sc: 60,
                open_timestamp_ms: 1672531320000,
                open: dec!(218),
                high: dec!(219),
                low: dec!(215),
                close: dec!(216),
                volume: dec!(12),
            },
            CandleData {
                symbol: "BTC-USDT".into(),
                interval_sc: 60,
                open_timestamp_ms: 1672531380000,
                open: dec!(216),
                high: dec!(217),
                low: dec!(212),
                close: dec!(213),
                volume: dec!(8),
            },
        ];
        let mut stream = stream::iter(all_candles);
        let candle = agg_candles_to_candle(&mut stream, 180)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(candle.open, dec!(200));
        assert_eq!(candle.high, dec!(220));
        assert_eq!(candle.low, dec!(190));
        assert_eq!(candle.close, dec!(216));
        assert_eq!(candle.volume, dec!(37));
        assert_eq!(candle.interval_sc, 180);
        assert_eq!(stream.next().await.unwrap().open, dec!(216));
        assert!(stream.next().await.is_none());
    }

    /// 测试流中数据不足以完成一次聚合的场景，应返回 None。
    #[tokio::test]
    async fn test_agg_candles_to_candle_incomplete() {
        let partial_candles: Vec<CandleData> = vec![
            CandleData {
                symbol: "BTC-USDT".into(),
                interval_sc: 60,
                open_timestamp_ms: 1672531200000,
                open: dec!(200),
                high: dec!(210),
                low: dec!(190),
                close: dec!(205),
                volume: dec!(10),
            },
            CandleData {
                symbol: "BTC-USDT".into(),
                interval_sc: 60,
                open_timestamp_ms: 1672531260000,
                open: dec!(205),
                high: dec!(220),
                low: dec!(202),
                close: dec!(218),
                volume: dec!(15),
            },
        ];
        let mut stream = stream::iter(partial_candles);
        let result = agg_candles_to_candle(&mut stream, 180).await.unwrap();
        assert!(
            result.is_none(),
            "Should return None for an incomplete group"
        );
    }

    /// 测试输入流为空的场景，应返回 None。
    #[tokio::test]
    async fn test_agg_candles_to_candle_empty_stream() {
        let empty_candles: Vec<CandleData> = vec![];
        let mut stream = stream::iter(empty_candles);
        let result = agg_candles_to_candle(&mut stream, 180).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_trade_to_candle_stream() {
        let trades: Vec<TradeData> = vec![
            // Candle #1 (10:00:00 -> 10:01:00)
            TradeData {
                symbol: "BTC-USDT".into(),
                timestamp_ms: 1756202405000,
                price: dec!(20000),
                quantity: dec!(1.5),
                side: Side::Buy,
            },
            TradeData {
                symbol: "BTC-USDT".into(),
                timestamp_ms: 1756202420000,
                price: dec!(19950),
                quantity: dec!(0.5),
                side: Side::Sell,
            },
            TradeData {
                symbol: "BTC-USDT".into(),
                timestamp_ms: 1756202455000,
                price: dec!(20100),
                quantity: dec!(2.0),
                side: Side::Buy,
            },
            // Candle #2 (10:02:00 -> 10:03:00)
            TradeData {
                symbol: "BTC-USDT".into(),
                timestamp_ms: 1756202525000,
                price: dec!(20120),
                quantity: dec!(3.0),
                side: Side::Buy,
            },
            // Candle #3 (10:03:00 -> 10:04:00)
            TradeData {
                symbol: "BTC-USDT".into(),
                timestamp_ms: 1756202580000,
                price: dec!(20150),
                quantity: dec!(1.0),
                side: Side::Sell,
            },
            TradeData {
                symbol: "BTC-USDT".into(),
                timestamp_ms: 1756202590000,
                price: dec!(20130),
                quantity: dec!(1.0),
                side: Side::Buy,
            },
        ];

        let trade_stream = stream::iter(trades);

        let candles: Vec<_> = transform_trades_to_candles(trade_stream, 60)
            .try_collect()
            .await
            .unwrap();

        println!("debug0: {candles:?}");

        assert_eq!(candles.len(), 3);

        let candle1 = &candles[0];
        assert_eq!(candle1.open_timestamp_ms, 1756202400000);
        assert_eq!(candle1.open, dec!(20000));
        assert_eq!(candle1.high, dec!(20100));
        assert_eq!(candle1.low, dec!(19950));
        assert_eq!(candle1.close, dec!(20100));
        assert_eq!(candle1.volume, dec!(4.0));

        let candle2 = &candles[1];
        assert_eq!(candle2.open_timestamp_ms, 1756202520000);
        assert_eq!(candle2.open, dec!(20120));
        assert_eq!(candle2.high, dec!(20120));
        assert_eq!(candle2.low, dec!(20120));
        assert_eq!(candle2.close, dec!(20120));
        assert_eq!(candle2.volume, dec!(3.0));

        let candle3 = &candles[2];
        assert_eq!(candle3.open_timestamp_ms, 1756202580000);
        assert_eq!(candle3.open, dec!(20150));
        assert_eq!(candle3.high, dec!(20150));
        assert_eq!(candle3.low, dec!(20130));
        assert_eq!(candle3.close, dec!(20130));
        assert_eq!(candle3.volume, dec!(2.0));
    }

    #[tokio::test]
    async fn test_candle_to_candle_stream() {
        let minute_candles: Vec<CandleData> = vec![
            // === 分组 1: 形成第一个3分钟K线 (时间窗口 00:00:00 -> 00:03:00) ===
            // 1分钟K线 (00:00:00 -> 00:01:00)
            CandleData {
                symbol: "BTC-USDT".into(),
                interval_sc: 60,
                open_timestamp_ms: 1672531200000, // 2023-01-01 00:00:00 UTC
                open: dec!(20000),
                high: dec!(20100),
                low: dec!(19950),
                close: dec!(20050),
                volume: dec!(10),
            },
            // 1分钟K线 (00:01:00 -> 00:02:00)
            CandleData {
                symbol: "BTC-USDT".into(),
                interval_sc: 60,
                open_timestamp_ms: 1672531260000,
                open: dec!(20050),
                high: dec!(20200),
                low: dec!(20040),
                close: dec!(20180),
                volume: dec!(15),
            },
            // 1分钟K线 (00:02:00 -> 00:03:00)
            CandleData {
                symbol: "BTC-USDT".into(),
                interval_sc: 60,
                open_timestamp_ms: 1672531320000,
                open: dec!(20180),
                high: dec!(20190),
                low: dec!(20150),
                close: dec!(20160),
                volume: dec!(12),
            },
            // === 分组 2: 形成第二个3分钟K线 (时间窗口 00:03:00 -> 00:06:00) ===
            // 1分钟K线 (00:03:00 -> 00:04:00)
            CandleData {
                symbol: "BTC-USDT".into(),
                interval_sc: 60,
                open_timestamp_ms: 1672531380000,
                open: dec!(20160),
                high: dec!(20170),
                low: dec!(20155),
                close: dec!(20165),
                volume: dec!(8),
            },
            // 1分钟K线 (00:04:00 -> 00:05:00)
            CandleData {
                symbol: "BTC-USDT".into(),
                interval_sc: 60,
                open_timestamp_ms: 1672531440000,
                open: dec!(20165),
                high: dec!(20180),
                low: dec!(20160),
                close: dec!(20175),
                volume: dec!(9),
            },
            // 1分钟K线 (00:05:00 -> 00:06:00)
            CandleData {
                symbol: "BTC-USDT".into(),
                interval_sc: 60,
                open_timestamp_ms: 1672531500000,
                open: dec!(20175),
                high: dec!(20185),
                low: dec!(20170),
                close: dec!(20180),
                volume: dec!(5),
            },
            // === 剩余数据: 这个K线不足以形成一个完整的组，将被丢弃 ===
            CandleData {
                symbol: "BTC-USDT".into(),
                interval_sc: 60,
                open_timestamp_ms: 1672531560000, // 00:06:00 -> 00:07:00
                open: dec!(20180),
                high: dec!(20190),
                low: dec!(20175),
                close: dec!(20185),
                volume: dec!(7),
            },
        ];

        let candle_iter = stream::iter(minute_candles);

        let aggregated_candles: Vec<_> = transform_candles_to_candles(candle_iter, 180)
            .try_collect()
            .await
            .unwrap();

        // 输入了7个1分钟K线，应该能聚合成2个完整的3分钟K线
        assert_eq!(
            aggregated_candles.len(),
            2,
            "It should be able to form two complete 3-minute K-line charts"
        );

        // 验证第一个聚合K线
        let agg1 = &aggregated_candles[0];
        assert_eq!(agg1.interval_sc, 180);
        assert_eq!(agg1.open_timestamp_ms, 1672531200000); // 应为第一组的开盘时间
        assert_eq!(agg1.open, dec!(20000)); // 第一根K线的开盘价
        assert_eq!(agg1.high, dec!(20200)); // 前三根K线中的最高价
        assert_eq!(agg1.low, dec!(19950)); // 前三根K线中的最低价
        assert_eq!(agg1.close, dec!(20160)); // 第三根K线的收盘价
        assert_eq!(agg1.volume, dec!(37)); // 10 + 15 + 12

        // 验证第二个聚合K线
        let agg2 = &aggregated_candles[1];
        assert_eq!(agg2.interval_sc, 180);
        assert_eq!(agg2.open_timestamp_ms, 1672531380000); // 应为第二组的开盘时间
        assert_eq!(agg2.open, dec!(20160)); // 第四根K线的开盘价
        assert_eq!(agg2.high, dec!(20185)); // 第4-6根K线中的最高价
        assert_eq!(agg2.low, dec!(20155)); // 第4-6根K线中的最低价
        assert_eq!(agg2.close, dec!(20180)); // 第六根K线的收盘价
        assert_eq!(agg2.volume, dec!(22)); // 8 + 9 + 5
    }
}
