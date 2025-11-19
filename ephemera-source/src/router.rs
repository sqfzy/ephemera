use dashmap::DashMap;
use ephemera_shared::{Exchange, MarketData, Symbol};
use flume::{Receiver, Sender, bounded};

pub struct Router {
    subscriptions: DashMap<RouterKey, Vec<Sender<MarketData>>>,
}

impl Router {
    pub fn new() -> Self {
        Self {
            subscriptions: DashMap::new(),
        }
    }

    /// 订阅数据流，返回接收端
    ///
    /// # 参数
    /// - `exchange`: 交易所名称（如 "binance"）
    /// - `symbol`: 交易对名称（如 "BTC/USDT"）
    /// - `buffer_size`: 通道缓冲区大小
    pub fn subscribe(&self, key: impl Into<RouterKey>, buffer_size: usize) -> Receiver<MarketData> {
        let (tx, rx) = bounded(buffer_size);

        self.subscriptions.entry(key.into()).or_default().push(tx);

        rx
    }

    /// 分发数据到所有订阅者
    #[inline]
    pub async fn dispatch(&self, key: &RouterKey, data: MarketData) {
        if let Some(senders) = self.subscriptions.get(key) {
            for tx in senders.value() {
                let _ = tx.send_async(data.clone()).await;
            }
        }
    }

    /// 获取订阅者数量
    pub fn subscriber_count(&self, key: &RouterKey) -> usize {
        self.subscriptions.get(key).map(|s| s.len()).unwrap_or(0)
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RouterKey {
    pub exchange: Exchange,
    pub symbol: Symbol,
}

impl RouterKey {
    pub fn new(exchange: impl Into<Exchange>, symbol: impl Into<Symbol>) -> Self {
        Self {
            exchange: exchange.into(),
            symbol: symbol.into(),
        }
    }
}

impl<T: Into<Exchange>, U: Into<Symbol>> From<(T, U)> for RouterKey {
    fn from((exchange, symbol): (T, U)) -> Self {
        Self::new(exchange, symbol)
    }
}
