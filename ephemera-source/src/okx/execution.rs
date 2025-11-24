use crate::okx::{
    OkxAuth,
    auth::signed_request,
    model::{HttpResponse, OrderInfo, PlaceOrderRequest},
};
use async_stream::stream;
use bytestring::ByteString;
use ephemera_shared::{OrderSide, OrderType, Signal, TradeMode};
use eyre::Result;
use futures::{Stream, StreamExt};
use reqwest::Method;
use std::pin::Pin;

/// 处理 API 响应
fn handle_http_response<T>(response: HttpResponse<T>) -> Result<T> {
    if response.code == "0" {
        response
            .data
            .into_iter()
            .next()
            .ok_or_else(|| eyre::eyre!("Empty response data"))
    } else {
        eyre::bail!("API Error: code={}, msg={}", response.code, response.msg)
    }
}

/// 下限价单
async fn place_limit_order(
    auth: &OkxAuth,
    symbol: impl Into<ByteString>,
    side: OrderSide,
    price: f64,
    size: f64,
) -> Result<OrderInfo> {
    let request = PlaceOrderRequest {
        inst_id: symbol.into(),
        td_mode: TradeMode::Cash,
        side,
        ord_type: OrderType::Limit,
        sz: size.to_string().into(),
        px: Some(price.to_string().into()),
    };

    let body = simd_json::serde::to_string(&request)?;
    let response: HttpResponse<OrderInfo> =
        signed_request(auth, Method::POST, "/api/v5/trade/order", &body).await?;

    handle_http_response(response)
}

/// 下市价单
async fn place_market_order(
    auth: &OkxAuth,
    symbol: impl Into<ByteString>,
    side: OrderSide,
    size: f64,
) -> Result<OrderInfo> {
    let request = PlaceOrderRequest {
        inst_id: symbol.into(),
        td_mode: TradeMode::Cash,
        side,
        ord_type: OrderType::Market,
        sz: size.to_string().into(),
        px: None,
    };

    let body = simd_json::serde::to_string(&request)?;
    let response: HttpResponse<OrderInfo> =
        signed_request(auth, Method::POST, "/api/v5/trade/order", &body).await?;

    handle_http_response(response)
}

/// 将信号流转换为订单执行流（限价单）
///
/// # 示例
/// ```no_run
/// use ephemera_source::okx::{okx_execute_limit_orders, OkxAuth};
/// use ephemera_shared::SignalWithSymbol;
/// use futures::{stream, StreamExt};
///
/// # async fn example() -> eyre::Result<()> {
/// let auth = OkxAuth::new("api_key", "secret_key", "passphrase")
///     .with_simulated(true);
///
/// let signals = stream::iter(vec![
///     SignalWithSymbol::buy("BTC-USDT".into(), 43000.0, 0.001),
/// ]);
///
/// let mut order_stream = okx_execute_limit_orders(auth, signals);
///
/// while let Some(result) = order_stream.next().await {
///     match result {
///         Ok(order) => println!("Order placed: {:?}", order),
///         Err(e) => eprintln!("Error: {}", e),
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub fn okx_execute_limit_orders(
    auth: OkxAuth,
    signal_stream: impl Stream<Item = Signal> + Send + 'static,
) -> Pin<Box<dyn Stream<Item = Result<OrderInfo>> + Send>> {
    let stream = stream! {
        futures::pin_mut!(signal_stream);

        while let Some(signal) = signal_stream.next().await {
            match signal {
                ephemera_shared::Signal::Buy { symbol, price, size } => {
                    tracing::info!(
                        "Executing BUY limit order: symbol={}, price={}, size={}",
                        symbol, price, size
                    );

                    match place_limit_order(&auth, symbol, OrderSide::Buy, price, size).await {
                        Ok(order) => yield Ok(order),
                        Err(e) => {
                            tracing::error!("Failed to place BUY order: {}", e);
                            yield Err(e);
                        }
                    }
                }
                ephemera_shared::Signal::Sell { symbol, price, size } => {
                    tracing::info!(
                        "Executing SELL limit order: symbol={}, price={}, size={}",
                        symbol, price, size
                    );

                    match place_limit_order(&auth, symbol, OrderSide::Sell, price, size).await {
                        Ok(order) => yield Ok(order),
                        Err(e) => {
                            tracing::error!("Failed to place SELL order: {}", e);
                            yield Err(e);
                        }
                    }
                }
                ephemera_shared::Signal::Hold => {
                    // 不执行任何操作
                }
            }
        }
    };

    Box::pin(stream)
}

/// 将信号流转换为订单执行流（市价单）
///
/// # 示例
/// ```no_run
/// use ephemera_source::okx::{okx_execute_market_orders, OkxAuth};
/// use ephemera_shared::SignalWithSymbol;
/// use futures::{stream, StreamExt};
///
/// # async fn example() -> eyre::Result<()> {
/// let auth = OkxAuth::new("api_key", "secret_key", "passphrase")
///     .with_simulated(true);
///
/// let signals = stream::iter(vec![
///     SignalWithSymbol::buy("BTC-USDT".into(), 43000.0, 0.001),
/// ]);
///
/// let mut order_stream = okx_execute_market_orders(auth, signals);
///
/// while let Some(result) = order_stream.next().await {
///     match result {
///         Ok(order) => println!("Order placed: {:?}", order),
///         Err(e) => eprintln!("Error: {}", e),
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub fn okx_execute_market_orders(
    auth: OkxAuth,
    signal_stream: impl Stream<Item = Signal> + Send + 'static,
) -> Pin<Box<dyn Stream<Item = Result<OrderInfo>> + Send>> {
    let stream = stream! {
        futures::pin_mut!(signal_stream);

        while let Some(signal) = signal_stream.next().await {
            match signal {
                ephemera_shared::Signal::Buy { symbol, price: _, size } => {
                    tracing::info!(
                        "Executing BUY market order: symbol={}, size={}",
                        symbol, size
                    );

                    match place_market_order(&auth, symbol, OrderSide::Buy, size).await {
                        Ok(order) => yield Ok(order),
                        Err(e) => {
                            tracing::error!("Failed to place BUY order: {}", e);
                            yield Err(e);
                        }
                    }
                }
                ephemera_shared::Signal::Sell { symbol, price: _, size } => {
                    tracing::info!(
                        "Executing SELL market order: symbol={}, size={}",
                        symbol, size
                    );

                    match place_market_order(&auth, symbol, OrderSide::Sell, size).await {
                        Ok(order) => yield Ok(order),
                        Err(e) => {
                            tracing::error!("Failed to place SELL order: {}", e);
                            yield Err(e);
                        }
                    }
                }
                ephemera_shared::Signal::Hold => {
                    // 不执行任何操作
                }
            }
        }
    };

    Box::pin(stream)
}
