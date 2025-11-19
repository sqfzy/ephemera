use chrono::{DateTime, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ephemera_source::okx;
use ephemera_shared::{BookData, CandleData, Symbol, TradeData};
use eyre::Result;
use futures::Stream;
use rust_decimal::prelude::ToPrimitive;
use std::{
    collections::{HashMap, VecDeque},
    pin::Pin,
    time::Instant,
};

const MAX_TRADES: usize = 100;
const MAX_CANDLES: usize = 100;
const MAX_LOGS: usize = 1000;
const MAX_PERFORMANCE_SAMPLES: usize = 100;
const RENDER_THROTTLE_MS: u128 = 50;
const LARGE_TRADE_THRESHOLD: f64 = 5.0; // 大额交易阈值

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Dashboard,
    Trades,
    Candles,
    OrderBook,
    Performance,
    Logs,
    Help,
}

impl Tab {
    pub fn next(&self) -> Self {
        match self {
            Tab::Dashboard => Tab::Trades,
            Tab::Trades => Tab::Candles,
            Tab::Candles => Tab::OrderBook,
            Tab::OrderBook => Tab::Performance,
            Tab::Performance => Tab::Logs,
            Tab::Logs => Tab::Help,
            Tab::Help => Tab::Dashboard,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Tab::Dashboard => Tab::Help,
            Tab::Trades => Tab::Dashboard,
            Tab::Candles => Tab::Trades,
            Tab::OrderBook => Tab::Candles,
            Tab::Performance => Tab::OrderBook,
            Tab::Logs => Tab::Performance,
            Tab::Help => Tab::Logs,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Tab::Dashboard => "Dashboard",
            Tab::Trades => "Trades",
            Tab::Candles => "Candles",
            Tab::OrderBook => "OrderBook",
            Tab::Performance => "Performance",
            Tab::Logs => "Logs",
            Tab::Help => "Help",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TradeStats {
    pub count: usize,
    pub volume: f64,
    pub last_price: f64,
    pub price_change_1m: f64,
    pub first_price: f64,
}

impl Default for TradeStats {
    fn default() -> Self {
        Self {
            count: 0,
            volume: 0.0,
            last_price: 0.0,
            price_change_1m: 0.0,
            first_price: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SystemStats {
    pub trades_per_sec: f64,
    pub candles_per_min: f64,
    pub peak_trades_per_sec: f64,
    pub avg_latency_ms: f64,
    pub connection_status: ConnectionStatus,
    pub total_bytes_received: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
    Reconnecting,
}

impl Default for SystemStats {
    fn default() -> Self {
        Self {
            trades_per_sec: 0.0,
            candles_per_min: 0.0,
            peak_trades_per_sec: 0.0,
            avg_latency_ms: 0.0,
            connection_status: ConnectionStatus::Disconnected,
            total_bytes_received: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PerformanceSample {
    pub timestamp: DateTime<Utc>,
    pub latency_ms: f64,
    pub trades_per_sec: f64,
    pub memory_mb: f64,
}

#[derive(Debug, Clone)]
pub struct AlertEvent {
    pub timestamp: DateTime<Utc>,
    pub message: String,
    pub severity: AlertSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

pub struct App {
    pub current_tab: Tab,
    pub symbols: Vec<Symbol>,
    pub selected_symbol_index: usize,
    pub trade_stream: Option<Pin<Box<dyn Stream<Item = Result<TradeData>> + Send>>>,
    pub candle_stream: Option<Pin<Box<dyn Stream<Item = Result<CandleData>> + Send>>>,
    pub book_stream: Option<Pin<Box<dyn Stream<Item = Result<BookData>> + Send>>>,
    pub recent_trades: VecDeque<TradeData>,
    pub candle_data: HashMap<Symbol, VecDeque<CandleData>>,
    pub order_books: HashMap<Symbol, BookData>,
    pub trade_stats: HashMap<Symbol, TradeStats>,
    pub system_stats: SystemStats,
    pub performance_history: VecDeque<PerformanceSample>,
    pub alerts: VecDeque<AlertEvent>,
    pub logs: VecDeque<String>,
    pub selected_trade_index: usize,
    pub help_scroll: u16,
    pub logs_scroll: u16,
    pub start_time: DateTime<Utc>,
    pub last_render: Instant,
    pub should_render: bool,
    pub trade_count: usize,
    pub candle_count: usize,
    pub last_stats_update: Instant,
    pub trades_in_last_second: usize,
    pub paused: bool,
    pub show_alerts: bool,
}

impl App {
    pub async fn new() -> Result<Self> {
        let symbols: Vec<Symbol> = vec!["BTC-USDT", "ETH-USDT", "SOL-USDT"]
            .into_iter()
            .map(|s| s.into())
            .collect();

        let mut app = Self {
            current_tab: Tab::Dashboard,
            symbols: symbols.clone(),
            selected_symbol_index: 0,
            trade_stream: None,
            candle_stream: None,
            book_stream: None,
            recent_trades: VecDeque::with_capacity(MAX_TRADES),
            candle_data: HashMap::new(),
            order_books: HashMap::new(),
            trade_stats: HashMap::new(),
            system_stats: SystemStats::default(),
            performance_history: VecDeque::with_capacity(MAX_PERFORMANCE_SAMPLES),
            alerts: VecDeque::with_capacity(100),
            logs: VecDeque::with_capacity(MAX_LOGS),
            selected_trade_index: 0,
            help_scroll: 0,
            logs_scroll: 0,
            start_time: Utc::now(),
            last_render: Instant::now(),
            should_render: true,
            trade_count: 0,
            candle_count: 0,
            last_stats_update: Instant::now(),
            trades_in_last_second: 0,
            paused: false,
            show_alerts: true,
        };

        app.add_log("🚀 Initializing Ephemera TUI...");

        // Initialize trade stats for all symbols
        for symbol in &symbols {
            app.trade_stats
                .insert(symbol.clone(), TradeStats::default());
        }

        // Initialize trade stream
        match okx::okx_trade_data_stream(symbols.clone()).await {
            Ok(stream) => {
                app.trade_stream = Some(Box::pin(stream));
                app.system_stats.connection_status = ConnectionStatus::Connected;
                app.add_log("🟢 Connected to OKX trade data stream");
            }
            Err(e) => {
                app.add_log(&format!("🔴 Failed to connect to OKX trade stream: {}", e));
            }
        }

        // Initialize candle stream
        match okx::okx_candle_data_stream(symbols.clone(), okx::OkxCandleInterval::Candle1s).await {
            Ok(stream) => {
                app.candle_stream = Some(Box::pin(stream));
                app.add_log("🟢 Connected to OKX candle data stream");
            }
            Err(e) => {
                app.add_log(&format!("🔴 Failed to connect to OKX candle stream: {}", e));
            }
        }

        // Initialize order book stream
        match okx::okx_book_data_stream(symbols.clone(), okx::OkxBookChannel::Books5).await {
            Ok(stream) => {
                app.book_stream = Some(Box::pin(stream));
                app.add_log("🟢 Connected to OKX order book stream");
            }
            Err(e) => {
                app.add_log(&format!("🔴 Failed to connect to OKX book stream: {}", e));
            }
        }

        app.add_log(&format!("📊 Monitoring symbols: {:?}", symbols));

        Ok(app)
    }

    pub fn handle_event(&mut self, event: KeyEvent) -> Result<bool> {
        match event.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                return Ok(true);
            }
            KeyCode::Tab => {
                if event.modifiers.contains(KeyModifiers::SHIFT) {
                    self.current_tab = self.current_tab.prev();
                } else {
                    self.current_tab = self.current_tab.next();
                }
                self.selected_trade_index = 0;
                self.should_render = true;
            }
            KeyCode::BackTab => {
                self.current_tab = self.current_tab.prev();
                self.selected_trade_index = 0;
                self.should_render = true;
            }
            KeyCode::Char('d') => {
                self.current_tab = Tab::Dashboard;
                self.should_render = true;
            }
            KeyCode::Char('t') => {
                self.current_tab = Tab::Trades;
                self.should_render = true;
            }
            KeyCode::Char('c') => {
                self.current_tab = Tab::Candles;
                self.should_render = true;
            }
            KeyCode::Char('o') => {
                self.current_tab = Tab::OrderBook;
                self.should_render = true;
            }
            KeyCode::Char('p') => {
                self.current_tab = Tab::Performance;
                self.should_render = true;
            }
            KeyCode::Char('l') => {
                self.current_tab = Tab::Logs;
                self.should_render = true;
            }
            KeyCode::Char('h') => {
                self.current_tab = Tab::Help;
                self.help_scroll = 0; // 重置滚动位置
                self.should_render = true;
            }
            KeyCode::Char('1') => {
                self.selected_symbol_index = 0;
                self.should_render = true;
            }
            KeyCode::Char('2') => {
                if self.symbols.len() > 1 {
                    self.selected_symbol_index = 1;
                    self.should_render = true;
                }
            }
            KeyCode::Char('3') => {
                if self.symbols.len() > 2 {
                    self.selected_symbol_index = 2;
                    self.should_render = true;
                }
            }
            KeyCode::Char(' ') => {
                self.paused = !self.paused;
                self.add_log(&format!(
                    "{}",
                    if self.paused {
                        "⏸ Data stream paused"
                    } else {
                        "▶ Data stream resumed"
                    }
                ));
                self.should_render = true;
            }
            KeyCode::Char('a') => {
                self.show_alerts = !self.show_alerts;
                self.should_render = true;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                // 根据当前 Tab 处理滚动
                match self.current_tab {
                    Tab::Help => {
                        if self.help_scroll > 0 {
                            self.help_scroll -= 1;
                            self.should_render = true;
                        }
                    }
                    Tab::Logs => {
                        if self.logs_scroll > 0 {
                            self.logs_scroll -= 1;
                            self.should_render = true;
                        }
                    }
                    Tab::Trades => {
                        if self.selected_trade_index > 0 {
                            self.selected_trade_index -= 1;
                            self.should_render = true;
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                // 根据当前 Tab 处理滚动
                match self.current_tab {
                    Tab::Help => {
                        self.help_scroll += 1;
                        self.should_render = true;
                    }
                    Tab::Logs => {
                        self.logs_scroll += 1;
                        self.should_render = true;
                    }
                    Tab::Trades => {
                        if self.selected_trade_index < self.recent_trades.len().saturating_sub(1) {
                            self.selected_trade_index += 1;
                            self.should_render = true;
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::PageUp => {
                // 快速向上滚动
                match self.current_tab {
                    Tab::Help => {
                        self.help_scroll = self.help_scroll.saturating_sub(10);
                        self.should_render = true;
                    }
                    Tab::Logs => {
                        self.logs_scroll = self.logs_scroll.saturating_sub(10);
                        self.should_render = true;
                    }
                    _ => {}
                }
            }
            KeyCode::PageDown => {
                // 快速向下滚动
                match self.current_tab {
                    Tab::Help => {
                        self.help_scroll += 10;
                        self.should_render = true;
                    }
                    Tab::Logs => {
                        self.logs_scroll += 10;
                        self.should_render = true;
                    }
                    _ => {}
                }
            }
            KeyCode::Home => {
                // 跳到顶部
                match self.current_tab {
                    Tab::Help => {
                        self.help_scroll = 0;
                        self.should_render = true;
                    }
                    Tab::Logs => {
                        self.logs_scroll = 0;
                        self.should_render = true;
                    }
                    _ => {}
                }
            }
            KeyCode::End => {
                // 跳到底部
                match self.current_tab {
                    Tab::Help => {
                        self.help_scroll = 1000; // 设置一个大数字，渲染时会被限制
                        self.should_render = true;
                    }
                    Tab::Logs => {
                        self.logs_scroll = 1000;
                        self.should_render = true;
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        Ok(false)
    }

    pub fn handle_trade_data(&mut self, result: Result<TradeData>) -> Result<()> {
        if self.paused {
            return Ok(());
        }

        match result {
            Ok(trade) => {
                self.trade_count += 1;
                self.trades_in_last_second += 1;

                // Update trade stats
                if let Some(stats) = self.trade_stats.get_mut(&trade.symbol) {
                    stats.count += 1;
                    if let (Some(price), Some(qty)) =
                        (trade.price.to_f64(), trade.quantity.to_f64())
                    {
                        // Set first price if not set
                        if stats.first_price == 0.0 {
                            stats.first_price = price;
                        }

                        let old_price = stats.last_price;
                        stats.last_price = price;
                        stats.volume += qty;

                        // Calculate price change from first price
                        if stats.first_price > 0.0 {
                            stats.price_change_1m =
                                ((price - stats.first_price) / stats.first_price) * 100.0;
                        }

                        // Detect large trades
                        if qty >= LARGE_TRADE_THRESHOLD {
                            let notional = price * qty;
                            self.add_alert(
                                &format!(
                                    "💰 Large trade: {} {:.2} @ ${:.2} (${:.0})",
                                    trade.symbol, qty, price, notional
                                ),
                                AlertSeverity::Info,
                            );
                        }

                        // Detect rapid price changes
                        if old_price > 0.0 {
                            let price_change_pct = ((price - old_price) / old_price) * 100.0;
                            if price_change_pct.abs() > 2.0 {
                                self.add_alert(
                                    &format!(
                                        "⚡ Price spike: {} {:+.2}% (${:.2} → ${:.2})",
                                        trade.symbol, price_change_pct, old_price, price
                                    ),
                                    AlertSeverity::Warning,
                                );
                            }
                        }
                    }
                }

                self.recent_trades.push_front(trade);
                if self.recent_trades.len() > MAX_TRADES {
                    self.recent_trades.pop_back();
                }

                self.mark_for_render();
            }
            Err(e) => {
                self.add_log(&format!("🔴 Trade data error: {}", e));
                self.add_alert(
                    &format!("Trade stream error: {}", e),
                    AlertSeverity::Critical,
                );
                self.should_render = true;
            }
        }
        Ok(())
    }

    pub fn handle_candle_data(&mut self, result: Result<CandleData>) -> Result<()> {
        if self.paused {
            return Ok(());
        }

        match result {
            Ok(candle) => {
                self.candle_count += 1;

                let candles = self
                    .candle_data
                    .entry(candle.symbol.clone())
                    .or_insert_with(|| VecDeque::with_capacity(MAX_CANDLES));
                candles.push_back(candle);
                if candles.len() > MAX_CANDLES {
                    candles.pop_front();
                }

                self.mark_for_render();
            }
            Err(e) => {
                self.add_log(&format!("🔴 Candle data error: {}", e));
                self.should_render = true;
            }
        }
        Ok(())
    }

    pub fn handle_book_data(&mut self, result: Result<BookData>) -> Result<()> {
        if self.paused {
            return Ok(());
        }

        match result {
            Ok(book) => {
                self.order_books.insert(book.symbol.clone(), book);
                self.mark_for_render();
            }
            Err(e) => {
                self.add_log(&format!("🔴 Order book error: {}", e));
                self.should_render = true;
            }
        }
        Ok(())
    }

    fn mark_for_render(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_render).as_millis() >= RENDER_THROTTLE_MS {
            self.should_render = true;
        }
    }

    pub fn can_render(&mut self) -> bool {
        if self.should_render {
            self.last_render = Instant::now();
            self.should_render = false;
            true
        } else {
            false
        }
    }

    pub fn add_log(&mut self, message: &str) {
        let timestamp = Utc::now().format("%H:%M:%S");
        let log_entry = format!("[{}] {}", timestamp, message);
        self.logs.push_front(log_entry);
        if self.logs.len() > MAX_LOGS {
            self.logs.pop_back();
        }
    }

    pub fn add_alert(&mut self, message: &str, severity: AlertSeverity) {
        let alert = AlertEvent {
            timestamp: Utc::now(),
            message: message.to_string(),
            severity,
        };
        self.alerts.push_front(alert);
        if self.alerts.len() > 100 {
            self.alerts.pop_back();
        }
    }

    pub fn on_tick(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_stats_update).as_secs_f64();

        if elapsed >= 1.0 {
            // Update system stats
            self.system_stats.trades_per_sec = self.trades_in_last_second as f64 / elapsed;
            self.system_stats.candles_per_min = (self.candle_count as f64 / elapsed) * 60.0;

            if self.system_stats.trades_per_sec > self.system_stats.peak_trades_per_sec {
                self.system_stats.peak_trades_per_sec = self.system_stats.trades_per_sec;
            }

            // Simulate latency (in real app, measure actual network latency)
            self.system_stats.avg_latency_ms = 15.0 + (rand::random::<f64>() * 10.0);

            // Add performance sample
            let sample = PerformanceSample {
                timestamp: Utc::now(),
                latency_ms: self.system_stats.avg_latency_ms,
                trades_per_sec: self.system_stats.trades_per_sec,
                memory_mb: 45.0 + (rand::random::<f64>() * 10.0), // Simulated
            };
            self.performance_history.push_back(sample);
            if self.performance_history.len() > MAX_PERFORMANCE_SAMPLES {
                self.performance_history.pop_front();
            }

            // Log statistics every 100 trades
            if self.trade_count > 0 && self.trade_count % 100 == 0 {
                self.add_log(&format!(
                    "📈 Stats: {} trades, {} candles, {:.1} trades/sec",
                    self.trade_count, self.candle_count, self.system_stats.trades_per_sec
                ));
            }

            self.trades_in_last_second = 0;
            self.last_stats_update = now;
            self.should_render = true;
        }
    }

    pub fn get_selected_symbol(&self) -> &Symbol {
        &self.symbols[self.selected_symbol_index]
    }

    pub fn get_selected_trade(&self) -> Option<&TradeData> {
        self.recent_trades.get(self.selected_trade_index)
    }
}
