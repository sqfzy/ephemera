use ephemera_shared::{CandleData, Signal, Symbol};
use ephemera_strategy::StrategyContext;
use std::collections::HashMap;

/// 回测结果
#[derive(Debug, Clone)]
pub struct BacktestResult {
    pub initial_balance: f64,
    pub final_balance: f64,
    pub total_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub total_return: f64,
    pub total_return_pct: f64,
    pub max_drawdown: f64,
    pub sharpe_ratio: f64,
    pub trades: Vec<Trade>,
}

#[derive(Debug, Clone)]
pub struct Trade {
    pub timestamp: u64,
    pub symbol: Symbol,
    pub side: TradeSide,
    pub price: f64,
    pub size: f64,
    pub balance_after: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TradeSide {
    Buy,
    Sell,
}

/// 简单的回测引擎
pub struct BacktestEngine {
    context: StrategyContext,
    initial_balance: f64,
    trades: Vec<Trade>,
    equity_curve: Vec<f64>,
    max_equity: f64,
}

impl BacktestEngine {
    pub fn new(initial_balance: f64) -> Self {
        Self {
            context: StrategyContext::new(initial_balance),
            initial_balance,
            trades: Vec::new(),
            equity_curve: vec![initial_balance],
            max_equity: initial_balance,
        }
    }

    /// 处理信号
    pub fn process_signal(&mut self, signal: Signal, candle: &CandleData) {
        match signal {
            Signal::Buy {
                symbol,
                price,
                size,
            } => {
                let cost = price * size;
                if self.context.available_balance >= cost {
                    // 执行买入
                    self.context.available_balance -= cost;
                    self.context.add_position(symbol.clone(), size, price);

                    self.trades.push(Trade {
                        timestamp: candle.open_timestamp_ms,
                        symbol,
                        side: TradeSide::Buy,
                        price,
                        size,
                        balance_after: self.get_total_equity(candle),
                    });
                }
            }
            Signal::Sell {
                symbol,
                price,
                size,
            } => {
                if let Some(position) = self.context.get_position(&symbol) {
                    let actual_size = size.min(position.size);
                    if actual_size > 0.0 {
                        // 执行卖出
                        self.context.reduce_position(&symbol, actual_size);
                        let revenue = price * actual_size;
                        self.context.available_balance += revenue;

                        self.trades.push(Trade {
                            timestamp: candle.open_timestamp_ms,
                            symbol,
                            side: TradeSide::Sell,
                            price,
                            size: actual_size,
                            balance_after: self.get_total_equity(candle),
                        });
                    }
                }
            }
            Signal::Hold => {}
        }

        // 更新权益曲线
        let equity = self.get_total_equity(candle);
        self.equity_curve.push(equity);
        self.max_equity = self.max_equity.max(equity);
    }

    /// 获取当前总权益
    fn get_total_equity(&self, candle: &CandleData) -> f64 {
        let mut equity = self.context.available_balance;

        if let Some(position) = self.context.get_position(&candle.symbol) {
            equity += position.value(candle.close);
        }

        equity
    }

    /// 生成回测报告
    pub fn generate_report(&self, last_candle: &CandleData) -> BacktestResult {
        let final_balance = self.get_total_equity(last_candle);
        let total_return = final_balance - self.initial_balance;
        let total_return_pct = (total_return / self.initial_balance) * 100.0;

        // 计算最大回撤
        let max_drawdown = self.calculate_max_drawdown();

        // 计算盈亏交易数
        let (winning_trades, losing_trades) = self.calculate_win_loss();

        // 计算夏普比率（简化版本）
        let sharpe_ratio = self.calculate_sharpe_ratio();

        BacktestResult {
            initial_balance: self.initial_balance,
            final_balance,
            total_trades: self.trades.len(),
            winning_trades,
            losing_trades,
            total_return,
            total_return_pct,
            max_drawdown,
            sharpe_ratio,
            trades: self.trades.clone(),
        }
    }

    fn calculate_max_drawdown(&self) -> f64 {
        let mut max_dd: f64 = 0.0;
        let mut peak = self.equity_curve[0];

        for &equity in &self.equity_curve {
            if equity > peak {
                peak = equity;
            }
            let dd = (peak - equity) / peak * 100.0;
            max_dd = max_dd.max(dd);
        }

        max_dd
    }

    fn calculate_win_loss(&self) -> (usize, usize) {
        let mut winning = 0;
        let mut losing = 0;
        let mut buy_trades: HashMap<Symbol, Vec<&Trade>> = HashMap::new();

        for trade in &self.trades {
            match trade.side {
                TradeSide::Buy => {
                    buy_trades
                        .entry(trade.symbol.clone())
                        .or_default()
                        .push(trade);
                }
                TradeSide::Sell => {
                    if let Some(buys) = buy_trades.get_mut(&trade.symbol) {
                        if let Some(buy) = buys.pop() {
                            if trade.price > buy.price {
                                winning += 1;
                            } else {
                                losing += 1;
                            }
                        }
                    }
                }
            }
        }

        (winning, losing)
    }

    fn calculate_sharpe_ratio(&self) -> f64 {
        if self.equity_curve.len() < 2 {
            return 0.0;
        }

        let returns: Vec<f64> = self
            .equity_curve
            .windows(2)
            .map(|w| (w[1] - w[0]) / w[0])
            .collect();

        let mean_return = returns.iter().sum::<f64>() / returns.len() as f64;
        let variance = returns
            .iter()
            .map(|r| (r - mean_return).powi(2))
            .sum::<f64>()
            / returns.len() as f64;
        let std_dev = variance.sqrt();

        if std_dev == 0.0 {
            0.0
        } else {
            mean_return / std_dev * (252.0_f64).sqrt() // 年化
        }
    }
}

impl BacktestResult {
    pub fn print_summary(&self) {
        println!("\n{:=<60}", "=");
        println!("回测结果摘要");
        println!("{:=<60}\n", "=");
        println!("初始资金: ${:.2}", self.initial_balance);
        println!("最终资金: ${:.2}", self.final_balance);
        println!("总收益: ${:.2}", self.total_return);
        println!("收益率: {:.2}%", self.total_return_pct);
        println!("最大回撤: {:.2}%", self.max_drawdown);
        println!("夏普比率: {:.2}", self.sharpe_ratio);
        println!("总交易次数: {}", self.total_trades);
        println!("盈利交易: {}", self.winning_trades);
        println!("亏损交易: {}", self.losing_trades);
        if self.winning_trades + self.losing_trades > 0 {
            let win_rate = self.winning_trades as f64
                / (self.winning_trades + self.losing_trades) as f64
                * 100.0;
            println!("胜率: {:.2}%", win_rate);
        }
        println!("{:=<60}\n", "=");
    }

    pub fn print_trades(&self, limit: Option<usize>) {
        println!("\n交易记录:");
        println!("{:-<100}", "");
        println!(
            "{:<20} {:<15} {:<8} {:<12} {:<10} {:<15}",
            "时间", "交易对", "方向", "价格", "数量", "账户余额"
        );
        println!("{:-<100}", "");

        let trades_to_show = if let Some(n) = limit {
            &self.trades[..n.min(self.trades.len())]
        } else {
            &self.trades
        };

        for trade in trades_to_show {
            let datetime = chrono::DateTime::from_timestamp_millis(trade.timestamp as i64)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "Invalid".to_string());

            println!(
                "{:<20} {:<15} {:<8} ${:<11.2} {:<10.4} ${:<14.2}",
                datetime,
                trade.symbol,
                if trade.side == TradeSide::Buy {
                    "买入"
                } else {
                    "卖出"
                },
                trade.price,
                trade.size,
                trade.balance_after
            );
        }
        println!("{:-<100}\n", "");
    }
}
