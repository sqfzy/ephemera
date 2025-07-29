use crate::{
    Timestamp,
    client::RawData,
    data::{BookData, CandleData, TradeData},
    order::Side,
    stream::IntoDataStream,
};
use async_stream::stream;
use bytestring::ByteString;
use eyre::{Context, ContextCompat, Result, ensure, eyre};
use futures::{SinkExt, Stream, StreamExt};
use http::StatusCode;
use reqwest::Client;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::pin::Pin;
use tokio_websockets::Message;
use url::Url;

pub(super) const OKX_CODE_SUCCESS: &str = "0";

#[derive(Debug, Clone)]
pub struct OkxHttpCandleDataRequest {
    pub end_point: ByteString,

    /// 产品ID，如 BTC-USDT
    pub inst_id: ByteString,
    /// 时间粒度，默认值1m
    /// 如 [1m/3m/5m/15m/30m/1H/2H/4H]
    /// 香港时间开盘价k线：[6H/12H/1D/2D/3D/1W/1M/3M]
    /// UTC时间开盘价k线：[/6Hutc/12Hutc/1Dutc/2Dutc/3Dutc/1Wutc/1Mutc/3Mutc]
    pub bar: Option<ByteString>,
    /// 请求此时间戳之前（更旧的数据）的分页内容，传的值为对应接口的ts
    pub after: Option<Timestamp>,
    /// 请求此时间戳之后（更新的数据）的分页内容，传的值为对应接口的ts, 单独使用时，会返回最新的数据。
    pub before: Option<Timestamp>,
    /// 分页返回的结果集数量，最大为300，不填默认返回100条
    pub limit: Option<usize>,
}

impl IntoDataStream for OkxHttpCandleDataRequest {
    type Error = eyre::Error;
    type Stream = Pin<Box<dyn Stream<Item = Result<CandleData, Self::Error>>>>;

    async fn into_stream(self) -> Result<Self::Stream, Self::Error> {
        let mut url = self.end_point.parse::<Url>()?;
        {
            let mut queries = url.query_pairs_mut();
            queries.append_pair("instId", &self.inst_id);

            if let Some(bar) = &self.bar {
                queries.append_pair("bar", bar);
            }
            if let Some(after) = self.after {
                queries.append_pair("after", &after.to_string());
            }
            if let Some(before) = self.before {
                queries.append_pair("before", &before.to_string());
            }
            if let Some(limit) = self.limit {
                queries.append_pair("limit", &limit.to_string());
            }
        }

        let client = Client::new();

        let stream = stream! {
            loop {
                let resp = client
                    .get(url.clone())
                    .send()
                    .await?
                    .error_for_status()?;

                match simd_json::from_slice::<OkxHttpResponse<OkxCandleData>>(&mut resp.bytes().await?.to_vec()) {
                    Ok(resp) => {
                        if resp.code != OKX_CODE_SUCCESS {
                            yield Err(eyre!("OKX error: {} - {}", resp.code, resp.msg));
                        }

                        for data in resp.data {
                            yield data.into_data()
                        }
                    },
                    Err(e) => yield Err(e.into()),

                }
            }
        };

        Ok(Box::pin(stream))
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct OkxHttpResponse<D> {
    pub code: ByteString,
    pub msg: ByteString,
    pub data: Vec<D>,
}

/// 订阅的频道
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OkxArg {
    /// 频道名
    /// candle3M
    /// candle1M
    /// candle1W
    /// candle1D
    /// candle2D
    /// candle3D
    /// candle5D
    /// candle12H
    /// candle6H
    /// candle4H
    /// candle2H
    /// candle1H
    /// candle30m
    /// candle15m
    /// candle5m
    /// candle3m
    /// candle1m
    /// candle1s
    /// candle3Mutc
    /// candle1Mutc
    /// candle1Wutc
    /// candle1Dutc
    /// candle2Dutc
    /// candle3Dutc
    /// candle5Dutc
    /// candle12Hutc
    /// candle6Hutc
    pub channel: ByteString,
    /// 产品ID，例如 "BTC-USDT"。
    pub inst_id: ByteString,
}

#[derive(Default, Serialize)]
pub struct OkxWsRequest<D> {
    #[serde(skip)]
    pub end_point: ByteString,

    #[serde(skip)]
    pub phantom: std::marker::PhantomData<D>,

    /// 操作
    /// subscribe
    /// unsubscribe
    pub op: ByteString,
    /// 请求订阅的频道列表
    pub args: Vec<OkxArg>,

    /// 消息的唯一标识。
    /// 用户提供，返回参数中会返回以便于找到相应的请求。
    /// 字母（区分大小写）与数字的组合，可以是纯字母、纯数字且长度必须要在1-32位之间。
    pub id: Option<ByteString>,
}

impl<D> IntoDataStream for OkxWsRequest<D>
where
    D: RawData<Error = eyre::Error> + DeserializeOwned + Send,
    D::Data: Send + 'static,
{
    type Error = eyre::Error;
    type Stream = Pin<Box<dyn Stream<Item = Result<D::Data, Self::Error>>>>;

    async fn into_stream(self) -> Result<Self::Stream, Self::Error> {
        let (mut client, upgrade_resp) = tokio_websockets::ClientBuilder::new()
            .uri(&self.end_point)?
            .connect()
            .await?;

        ensure!(
            upgrade_resp.status() == StatusCode::SWITCHING_PROTOCOLS,
            "WebSocket connection failed: {}",
            upgrade_resp.status(),
        );

        client
            .send(Message::text(simd_json::serde::to_string(&self)?))
            .await?;

        let resp = simd_json::from_slice::<OkxWsResponse>(
            &mut client
                .next()
                .await
                .wrap_err("Failed to subscribe")??
                .as_payload()
                .to_vec(),
        )?;
        ensure!(
            resp.event == "subscribe",
            "{}: {}",
            resp.code.unwrap_or_default(),
            resp.msg.unwrap_or_default()
        );

        let stream = stream! {
            while let Some(msg) = client.next().await {
                let msg = msg?;
                match simd_json::from_slice::<OkxWsDataResponse<D>>(&mut msg.as_payload().to_vec()) {
                    Ok(resp) => {
                        for data in resp.data {
                            yield data.into_data()
                        }
                    },
                    Err(e) => yield Err(e.into()),

                }
            }
        };

        Ok(Box::pin(stream))
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OkxWsResponse {
    /// 消息的唯一标识。
    pub id: Option<ByteString>,

    /// 事件类型
    pub event: ByteString,

    /// 错误码
    pub code: Option<ByteString>,

    /// 错误消息
    pub msg: Option<ByteString>,

    /// 订阅的频道
    pub arg: Option<OkxArg>,

    /// WebSocket连接ID
    pub conn_id: ByteString,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OkxWsDataResponse<D> {
    pub arg: OkxArg,
    pub action: Option<ByteString>,
    pub data: Vec<D>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OkxTradeData {
    pub inst_id: ByteString,
    pub trade_id: ByteString,
    pub px: ByteString,
    pub sz: ByteString,
    pub side: ByteString,
    pub ts: ByteString,
}

impl RawData for OkxTradeData {
    type Error = eyre::Error;
    type Data = TradeData;

    fn into_data(self) -> Result<Self::Data, Self::Error> {
        let timestamp = self.ts.parse()?;
        let price = self.px.parse::<f64>()?;
        let quantity = self.sz.parse::<f64>()?;
        let side = Side::try_from(self.side.as_ref())?;

        Ok(Self::Data {
            trade_id: self.trade_id,
            symbol: self.inst_id,
            price,
            quantity,
            side,
            timestamp,
        })
    }
}

/// 0. 价格,
/// 1. 数量,
/// 2. 流动性订单数量,
/// 3. 订单数量
pub type Level = (ByteString, ByteString, ByteString, ByteString);

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OkxBookData {
    pub asks: Vec<Level>,
    pub bids: Vec<Level>,
    pub ts: ByteString,

    /// 检验和
    pub checksum: Option<i128>,

    /// 上一个推送的序列号。仅适用 books，books-l2-tbt，books50-l2-tbt
    pub prev_seq_id: Option<i128>,

    /// 推送的序列号
    pub seq_id: Option<i128>,
}

impl RawData for OkxBookData {
    type Error = eyre::Error;
    type Data = BookData;

    fn into_data(self) -> Result<Self::Data, Self::Error> {
        let parse_levels = |levels: &Vec<Level>| -> Result<Vec<(f64, f64)>> {
            levels
                .iter()
                .map(|(price_str, size_str, _, _)| {
                    let price = price_str.parse::<f64>()?;
                    let size = size_str.parse::<f64>()?;
                    Ok((price, size))
                })
                .collect()
        };

        let timestamp = self.ts.parse().wrap_err("Failed to parse book timestamp")?;
        let bids = parse_levels(&self.bids).wrap_err("Failed to parse bids")?;
        let asks = parse_levels(&self.asks).wrap_err("Failed to parse asks")?;

        Ok(Self::Data {
            timestamp,
            bids,
            asks,
        })
    }
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
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct OkxCandleData(
    ByteString,
    ByteString,
    ByteString,
    ByteString,
    ByteString,
    ByteString,
    ByteString,
    ByteString,
    ByteString,
);

impl RawData for OkxCandleData {
    type Error = eyre::Error;
    type Data = CandleData;

    fn into_data(self) -> Result<Self::Data, Self::Error> {
        let timestamp = self
            .0
            .parse()
            .wrap_err("Failed to parse candle timestamp")?;
        let open = self
            .1
            .parse::<f64>()
            .wrap_err("Failed to parse open price")?;
        let high = self
            .2
            .parse::<f64>()
            .wrap_err("Failed to parse high price")?;
        let low = self
            .3
            .parse::<f64>()
            .wrap_err("Failed to parse low price")?;
        let close = self
            .4
            .parse::<f64>()
            .wrap_err("Failed to parse close price")?;
        let volume = self.5.parse::<f64>().wrap_err("Failed to parse volume")?;

        Ok(Self::Data {
            timestamp,
            open,
            high,
            low,
            close,
            volume,
        })
    }
}
