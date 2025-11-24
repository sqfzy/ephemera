pub mod auth;
pub mod execution;
pub mod fetch;

mod model;

pub use auth::{OkxAuth, okx_verified_auth_stream};
pub use execution::{okx_execute_limit_orders, okx_execute_market_orders};
pub use fetch::{
    OkxBookChannel, OkxCandleInterval, okx_xdp_book_data_stream, okx_xdp_candle_data_stream,
    okx_xdp_trade_data_stream,
};
pub use model::{OrderInfo, WsOperation};

pub(super) const OKX_REST_API_BASE: &str = "https://www.okx.com";
pub(super) const OKX_WS_HOST: &str = "ws.okx.com:8443";
pub(super) const OKX_WS_PUBLICE_ENDPOINT: &str = "wss://ws.okx.com:8443/ws/v5/public";
pub(super) const OKX_WS_BUSINESS_ENDPOINT: &str = "wss://ws.okx.com:8443/ws/v5/business";
