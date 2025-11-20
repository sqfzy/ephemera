use super::*;
use ephemera_xdp::async_stream::XdpTcpStream;

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
    println!("debug0");
    let stream = XdpTcpStream::connect(OKX_WS_HOST).await?;
    println!("debug1");
    okx_raw_data_stream::<WsDataResponse<RawTradeData>>(OKX_WS_PUBLICE_ENDPOINT, request, stream)
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
    let stream = XdpTcpStream::connect(OKX_WS_HOST).await?;
    okx_raw_data_stream::<WsDataResponse<RawCandleData>>(OKX_WS_BUSINESS_ENDPOINT, request, stream)
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
    let stream = XdpTcpStream::connect(OKX_WS_HOST).await?;
    okx_raw_data_stream::<WsDataResponse<OkxBookData>>(OKX_WS_PUBLICE_ENDPOINT, request, stream)
        .await
        .map(transform_raw_vec_stream)
}

#[cfg(test)]
#[serial_test::serial]
mod tests {
    use super::*;
    use ephemera_shared::Symbol;
    use std::sync::OnceLock;

    const SYMBOLS: [Symbol; 2] = [
        Symbol::from_static("BTC-USDT"),
        Symbol::from_static("ETH-USDT"),
    ];
    const TEST_DATA_NUM: usize = 3;

    fn setup() {
        static START: OnceLock<()> = OnceLock::new();

        START.get_or_init(|| {
            let level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());

            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::from_str(&level).unwrap())
                .init();
        });
    }

    #[tokio::test]
    async fn test_okx_xdp_trade_data_stream() {
        setup();
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
        setup();
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
        setup();
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
