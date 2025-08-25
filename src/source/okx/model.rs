#![allow(dead_code)]

use crate::{
    Timestamp,
    data::{BookData, TradeData,Side},
};
use bytestring::ByteString;
use eyre::Result;
use itertools::Itertools;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use strum::{AsRefStr, Display, EnumString};

#[derive(Debug, Clone, Serialize)]
pub(super) struct HttpCandleDataRequest {
    /// 产品ID，如 BTC-USDT
    pub(super) inst_id: ByteString,
    /// 时间粒度，默认值1m
    /// 如 [1m/3m/5m/15m/30m/1H/2H/4H]
    /// 香港时间开盘价k线：[6H/12H/1D/2D/3D/1W/1M/3M]
    /// UTC时间开盘价k线：[/6Hutc/12Hutc/1Dutc/2Dutc/3Dutc/1Wutc/1Mutc/3Mutc]
    pub(super) bar: Option<ByteString>,
    /// 请求此时间戳之前（更旧的数据）的分页内容，传的值为对应接口的ts
    pub(super) after: Option<Timestamp>,
    /// 请求此时间戳之后（更新的数据）的分页内容，传的值为对应接口的ts, 单独使用时，会返回最新的数据。
    pub(super) before: Option<Timestamp>,
    /// 分页返回的结果集数量，最大为300，不填默认返回100条
    pub(super) limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct HttpResponse<RD> {
    pub(super) code: ByteString,
    pub(super) msg: ByteString,
    pub(super) data: Vec<RD>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct WsRequest {
    /// 操作
    /// subscribe
    /// unsubscribe
    pub(super) op: WsOperation,

    /// 请求订阅的频道列表
    pub(super) args: Vec<Arg>,

    /// 消息的唯一标识。
    /// 用户提供，返回参数中会返回以便于找到相应的请求。
    /// 字母（区分大小写）与数字的组合，可以是纯字母、纯数字且长度必须要在1-32位之间。
    pub(super) id: Option<ByteString>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WsResponse {
    /// 消息的唯一标识。
    pub(super) id: Option<ByteString>,

    /// 事件类型
    pub(super) event: ByteString,

    /// 错误码
    pub(super) code: Option<ByteString>,

    /// 错误消息
    pub(super) msg: Option<ByteString>,

    /// 订阅的频道
    pub(super) arg: Option<Arg>,

    /// WebSocket连接ID
    pub(super) conn_id: ByteString,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WsDataResponse<RD> {
    pub(super) arg: Arg,
    pub(super) action: Option<ByteString>,
    pub(super) data: Vec<RD>,
}

impl TryFrom<WsDataResponse<RawTradeData>> for Vec<TradeData> {
    type Error = eyre::Error;

    fn try_from(value: WsDataResponse<RawTradeData>) -> std::result::Result<Self, Self::Error> {
        value
            .data
            .into_iter()
            .map(|trade| {
                let trade_id = trade.trade_id.parse::<u64>()?;
                let timestamp = trade.ts.parse()?;
                let price = trade.px.parse::<Decimal>()?;
                let quantity = trade.sz.parse::<Decimal>()?;
                let side = Side::try_from(trade.side.as_ref())?;

                Ok(TradeData {
                    trade_id,
                    symbol: value.arg.inst_id.clone(),
                    price,
                    quantity,
                    side,
                    timestamp,
                })
            })
            .try_collect()
    }
}

impl TryFrom<WsDataResponse<OkxBookData>> for Vec<BookData> {
    type Error = eyre::Error;

    fn try_from(value: WsDataResponse<OkxBookData>) -> Result<Self, Self::Error> {
        let parse_levels = |levels: Vec<Level>| -> Result<Vec<(Decimal, Decimal)>> {
            levels
                .into_iter()
                .map(|(price_str, size_str, _, _)| {
                    let price = price_str.parse::<Decimal>()?;
                    let size = size_str.parse::<Decimal>()?;
                    Ok((price, size))
                })
                .collect()
        };

        let symbol = value.arg.inst_id;

        value
            .data
            .into_iter()
            .map(|value| {
                let timestamp = value.ts.parse()?;
                let bids = parse_levels(value.bids)?;
                let asks = parse_levels(value.asks)?;

                Ok(BookData {
                    symbol: symbol.clone(),
                    timestamp,
                    bids,
                    asks,
                })
            })
            .try_collect()
    }
}

/// 订阅的频道
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Arg {
    /// 产品ID，例如 "BTC-USDT"。
    pub(super) inst_id: ByteString,

    /// 频道名，例如：
    /// candle1D,
    /// tickers,
    /// trades
    pub(super) channel: ByteString,
}

impl Arg {
    pub(super) fn new(channel: impl Into<ByteString>, inst_id: impl Into<ByteString>) -> Self {
        Self {
            channel: channel.into(),
            inst_id: inst_id.into(),
        }
    }
}

impl<T: Into<ByteString>> From<(T, T)> for Arg {
    fn from((channel, inst_id): (T, T)) -> Self {
        Self::new(channel, inst_id)
    }
}

#[derive(
    Debug, PartialEq, Eq, Clone, Copy, Display, EnumString, AsRefStr, Serialize, Deserialize,
)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum WsOperation {
    Subscribe,
    Unsubscribe,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawTradeData {
    pub(super) inst_id: ByteString,
    pub(super) trade_id: ByteString,
    pub(super) px: ByteString,
    pub(super) sz: ByteString,
    pub(super) side: ByteString,
    pub(super) ts: ByteString,
}

/// 0.开始时间，Unix时间戳的毫秒数
/// 1.开盘价
/// 2.最高价
/// 3.最低价
/// 4.收盘价
/// 5.交易量（以币为单位）
/// 6.交易量（以计价货币为单位）
/// 7.交易量（以计价货币为单位，适用于合约）
/// 8.K线状态 (1: a confirmed candle)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct RawCandleData(
    pub(super) ByteString,
    pub(super) ByteString,
    pub(super) ByteString,
    pub(super) ByteString,
    pub(super) ByteString,
    pub(super) ByteString,
    pub(super) ByteString,
    pub(super) ByteString,
    pub(super) ByteString,
);

/// 0. 价格,
/// 1. 数量,
/// 2. 流动性订单数量,
/// 3. 订单数量
pub(super) type Level = (ByteString, ByteString, ByteString, ByteString);

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OkxBookData {
    pub(super) asks: Vec<Level>,
    pub(super) bids: Vec<Level>,
    pub(super) ts: ByteString,

    /// 检验和
    pub(super) checksum: Option<i128>,

    /// 上一个推送的序列号。仅适用 books，books-l2-tbt，books50-l2-tbt
    pub(super) prev_seq_id: Option<i128>,

    /// 推送的序列号
    pub(super) seq_id: Option<i128>,
}
