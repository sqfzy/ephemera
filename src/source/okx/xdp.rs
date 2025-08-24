use super::*;
use crate::xdp::async_stream::XdpTcpStream;
use http::Uri;
use std::str::FromStr;

pub async fn okx_xdp_trade_data_stream(
    symbols: Vec<impl Into<ByteString>>,
) -> eyre::Result<impl Stream<Item = Result<TradeData>>> {
    let request = WsRequest {
        op: WsOperation::Subscribe,
        args: symbols
            .into_iter()
            .map(|inst_id| Arg::new(ByteString::from_static("trades"), inst_id.into()))
            .collect_vec(),
        id: None,
    };
    okx_xdp_raw_data_stream::<WsDataResponse<RawTradeData>>(OKX_WS_PUBLICE_ENDPOINT, request)
        .await
        .map(transform_raw_vec_stream)
}

pub async fn okx_xdp_candle_data_stream(
    symbols: Vec<impl Into<ByteString>>,
    interval: OkxCandleInterval,
) -> eyre::Result<impl Stream<Item = Result<CandleData>>> {
    let request = WsRequest {
        op: WsOperation::Subscribe,
        args: symbols
            .into_iter()
            .map(|inst_id| {
                Arg::new(
                    ByteString::from_static(interval.clone().into()),
                    inst_id.into(),
                )
            })
            .collect_vec(),
        id: None,
    };
    okx_xdp_raw_data_stream::<WsDataResponse<RawCandleData>>(OKX_WS_BUSINESS_ENDPOINT, request)
        .await
        .map(move |stream| {
            transform_raw_vec_stream_with(stream, move |resp| {
                convert_okx_candle_datas(resp, interval.clone().into())
            })
        })
}

pub async fn okx_xdp_book_data_stream(
    symbols: Vec<impl Into<ByteString>>,
    typ: OkxBookChannel,
) -> eyre::Result<impl Stream<Item = Result<BookData>>> {
    let request = WsRequest {
        op: WsOperation::Subscribe,
        args: symbols
            .into_iter()
            .map(|inst_id| Arg::new(ByteString::from_static(typ.clone().into()), inst_id.into()))
            .collect_vec(),
        id: None,
    };
    okx_xdp_raw_data_stream::<WsDataResponse<OkxBookData>>(OKX_WS_PUBLICE_ENDPOINT, request)
        .await
        .map(transform_raw_vec_stream)
}

// TODO: 返回sink和stream
async fn okx_xdp_raw_data_stream<DR: DeserializeOwned + Send + 'static>(
    end_point: &str,
    request: WsRequest,
) -> Result<Pin<Box<dyn Stream<Item = Result<DR, eyre::Error>> + Send>>, eyre::Error> {
    let channel_count = request.args.len();

    assert_ne!(
        channel_count, 0,
        "At least one channel must be specified for subscription"
    );

    let uri = Uri::from_str(end_point)?;
    let stream = XdpTcpStream::connect((uri.host().unwrap(), uri.port_u16().unwrap())).await?;

    let (mut client, upgrade_resp) = tokio_websockets::ClientBuilder::from_uri(uri)
        .connect_on(stream)
        .await?;

    ensure!(
        upgrade_resp.status() == StatusCode::SWITCHING_PROTOCOLS,
        "WebSocket connection failed: {}",
        upgrade_resp.status(),
    );

    client
        .send(Message::text(simd_json::serde::to_string(&request)?))
        .await?;

    // Each channel subscription will get a response.
    for _ in 0..channel_count {
        // Expect a response like this:
        // {
        //   "event": "subscribe",
        //   "arg": {
        //     "channel": "trades",
        //     "instId": "BTC-USDT"
        //   },
        //   "id": "user_sub_01"
        // }
        let mut resp = client
            .next()
            .await
            .wrap_err("Failed to subscribe")??
            .as_payload()
            .to_vec();
        let resp = simd_json::from_slice::<WsResponse>(&mut resp)?;
        ensure!(
            resp.event == WsOperation::Subscribe,
            "Failed to subscribe with response:\n {resp:?}",
        );
    }

    let stream = stream! {
        while let Some(msg) = client.next().await {
            let msg = msg?;
            match simd_json::from_slice::<DR>(&mut msg.as_payload().to_vec()) {
                Ok(resp) => yield Ok(resp),
                Err(e) => yield Err(e.into()),

            }
        }
    };

    Ok(Box::pin(stream))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Symbol;

    const SYMBOLS: [Symbol; 2] = [
        Symbol::from_static("BTC-USDT"),
        Symbol::from_static("ETH-USDT"),
    ];
    const TEST_DATA_NUM: usize = 5;

    #[tokio::test]
    async fn test_okx_xdp_trade_data_stream() {
        okx_xdp_trade_data_stream(SYMBOLS.to_vec())
            .await
            .unwrap()
            .take(TEST_DATA_NUM)
            .for_each(|res| {
                assert!(SYMBOLS.contains(&res.unwrap().symbol));
                std::future::ready(())
            })
            .await;
    }

    #[tokio::test]
    async fn test_xdp_okx_candle_data_stream() {
        okx_xdp_candle_data_stream(SYMBOLS.to_vec(), OkxCandleInterval::Candle1s)
            .await
            .unwrap()
            .take(TEST_DATA_NUM)
            .for_each(|res| {
                assert!(SYMBOLS.contains(&res.unwrap().symbol));
                std::future::ready(())
            })
            .await;
    }

    #[tokio::test]
    async fn test_xdp_okx_book_data_stream() {
        okx_xdp_book_data_stream(SYMBOLS.to_vec(), OkxBookChannel::BboTbt)
            .await
            .unwrap()
            .take(TEST_DATA_NUM)
            .for_each(|res| {
                assert!(SYMBOLS.contains(&res.unwrap().symbol));
                std::future::ready(())
            })
            .await;
    }
}
