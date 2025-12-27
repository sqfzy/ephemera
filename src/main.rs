use ephemera_shared::{CandleData, Signal};
use ephemera_source::csv::csv_candle_data_stream;
use ephemera_source::okx::{
    OkxAuth, OkxCandleInterval, OrderInfo, okx_execute_market_orders, okx_xdp_candle_data_stream,
};
use ephemera_strategy::strategies::{
    CircuitBreakerConfig, LeverageConfig, MACrossStrategy, ScalpingStrategy, SlippageModel,
    Strategy,
};
use eyre::Result;
use futures::{Stream, StreamExt};
use std::pin::Pin;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv();

    // 初始化日志
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("🚀 Ephemera 交易系统\n");

    // 从环境变量选择模式
    let mode = std::env::var("MODE").unwrap_or_else(|_| "backtest".to_string());

    match mode.as_str() {
        "backtest" => run_backtest().await?,
        "live" => run_live_trading().await?,
        _ => {
            eprintln!("❌ 未知模式: {}. 请使用 'backtest' 或 'live'", mode);
            std::process::exit(1);
        }
    }

    Ok(())
}

/// 运行回测
async fn run_backtest() -> Result<()> {
    println!("📊 运行回测模式\n");

    // 配置参数
    let data_path = "data/binance_btc-usdt_1m.csv";
    let symbol = "BTC-USDT";
    let initial_balance = 10000.0;
    let position_size = 0.01;
    let fast_period = 5;
    let slow_period = 20;

    println!("配置参数:");
    println!("  数据文件: {}", data_path);
    println!("  交易对: {}", symbol);
    println!("  初始资金: {} USDT", initial_balance);
    println!("  策略: 双均线交叉 (MA{}/MA{})", fast_period, slow_period);
    println!("  仓位大小: {} BTC\n", position_size);

    // 创建数据流
    let candle_stream = csv_candle_data_stream(data_path).await?;

    // 创建策略
    let strategy = ScalpingStrategy::new(
        symbol.into(),
        20,                        // 布林带周期
        2.0,                       // 布林带标准差
        5,                         // 快速 EMA
        10,                        // 慢速 EMA
        0.01,                      // 仓位大小
        2.0,                       // 2% 止盈（杠杆放大后）
        1.0,                       // 1% 止损（杠杆放大后）
        LeverageConfig::new(20.0), // 20x 杠杆
        SlippageModel::Dynamic {
            base_slippage: 0.1, // 基础 0.1% 滑点
            volume_factor: 0.5, // 成交量调整因子
        },
        CircuitBreakerConfig {
            max_consecutive_losses: 3, // 连续 3 次亏损熔断
            daily_max_loss_pct: 10.0,  // 单日最大 10% 亏损
            single_max_loss_pct: 3.0,  // 单笔最大 3% 亏损
            volatility_threshold: 5.0, // 5% 波动率警告
            cooldown_candles: 20,      // 熔断后冷却 20 根 K线
        },
    );

    // 组合 Stream：数据流 -> 策略流 -> 信号流
    let signal_stream = apply_strategy(candle_stream, strategy);

    // 执行回测并收集结果
    let report = execute_backtest(signal_stream, initial_balance).await?;

    // 打印报告
    print_backtest_report(&report);
    print_trades(&report.trades, Some(20));

    Ok(())
}

/// 运行实盘交易
async fn run_live_trading() -> Result<()> {
    println!("🔴 运行实盘交易模式（模拟盘）\n");

    // OKX API 配置
    let api_key = std::env::var("OKX_API_KEY")?;
    let secret_key = std::env::var("OKX_SECRET_KEY")?;
    let passphrase = std::env::var("OKX_PASSPHRASE")?;

    let auth = OkxAuth::new(api_key, secret_key, passphrase).with_simulated(true);

    println!("✅ OKX 认证配置完成（模拟交易模式）\n");

    // 配置参数
    let symbol = "BTC-USDT";
    let position_size = 0.001;
    let fast_period = 5;
    let slow_period = 20;

    println!("配置参数:");
    println!("  交易对: {}", symbol);
    println!("  策略: 双均线交叉 (MA{}/MA{})", fast_period, slow_period);
    println!("  仓位大小: {} BTC\n", position_size);

    // 创建数据流 - 修复：明确指定类型为 ByteString
    let candle_stream = okx_xdp_candle_data_stream(vec![symbol], OkxCandleInterval::Min1).await?;

    println!("✅ 成功连接到 OKX 数据流\n");

    // 创建策略
    let strategy = MACrossStrategy::new(symbol.into(), fast_period, slow_period, position_size);

    // 组合 Stream：数据流 -> 策略流 -> 信号流 -> 订单执行流
    let signal_stream = apply_strategy(candle_stream, strategy);

    // 只提取 Signal，不包含 CandleData
    let signal_only_stream = extract_signals(signal_stream);

    let order_stream = okx_execute_market_orders(auth, signal_only_stream);

    // 消费订单流
    consume_order_stream(order_stream).await?;

    Ok(())
}

/// 将策略应用到数据流，生成信号流
fn apply_strategy<S>(
    candle_stream: impl Stream<Item = Result<CandleData>> + Send + 'static,
    mut strategy: S,
) -> Pin<Box<dyn Stream<Item = (Signal, CandleData)> + Send>>
where
    S: Strategy<Input = CandleData, Signal = Signal> + Send + 'static,
    S::Error: std::fmt::Debug + Send, // 添加 Send 约束
{
    Box::pin(async_stream::stream! {
        futures::pin_mut!(candle_stream);

        let mut count = 0;

        while let Some(result) = candle_stream.next().await {
            match result {
                Ok(candle) => {
                    count += 1;

                    if count % 100 == 0 {
                        tracing::info!("已处理 {} 根K线...", count);
                    }

                    match strategy.on_data(candle.clone()).await {
                        Ok(Some(signal)) => {
                            yield (signal, candle);
                        }
                        Ok(None) => {
                            // 策略还在预热，没有信号
                        }
                        Err(e) => {
                            tracing::error!("策略处理错误: {:?}", e);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("读取K线数据错误: {}", e);
                    break;
                }
            }
        }

        tracing::info!("✅ 数据处理完成，共处理 {} 根K线", count);
    })
}

/// 从信号流中只提取 Signal（用于实盘交易）
fn extract_signals(
    signal_stream: impl Stream<Item = (Signal, CandleData)> + Send + 'static,
) -> Pin<Box<dyn Stream<Item = Signal> + Send>> {
    Box::pin(async_stream::stream! {
        futures::pin_mut!(signal_stream);

        while let Some((signal, _candle)) = signal_stream.next().await {
            yield signal;
        }
    })
}

/// 执行回测，返回回测报告
async fn execute_backtest(
    signal_stream: impl Stream<Item = (Signal, CandleData)> + Send,
    initial_balance: f64,
) -> Result<BacktestReport> {
    use std::collections::HashMap;

    let mut available_balance = initial_balance;
    let mut positions: HashMap<String, Position> = HashMap::new();
    let mut trades = Vec::new();
    let mut equity_curve = vec![initial_balance];
    let mut max_equity = initial_balance;

    futures::pin_mut!(signal_stream);

    while let Some((signal, candle)) = signal_stream.next().await {
        match signal {
            Signal::Buy {
                symbol,
                price,
                size,
            } => {
                let cost = price * size;
                if available_balance >= cost {
                    available_balance -= cost;

                    let position = positions.entry(symbol.to_string()).or_insert(Position {
                        size: 0.0,
                        avg_price: 0.0,
                    });

                    if position.size == 0.0 {
                        position.avg_price = price;
                        position.size = size;
                    } else {
                        let total_cost = position.avg_price * position.size + price * size;
                        position.size += size;
                        position.avg_price = total_cost / position.size;
                    }

                    let equity = calculate_equity(available_balance, &positions, &candle);
                    equity_curve.push(equity);
                    max_equity = max_equity.max(equity);

                    trades.push(Trade {
                        timestamp: candle.open_timestamp_ms,
                        symbol: symbol.to_string(),
                        side: TradeSide::Buy,
                        price,
                        size,
                        balance_after: equity,
                    });

                    tracing::info!(
                        "📈 买入: {} @ {:.2}, 数量: {:.4}, 余额: {:.2}",
                        symbol,
                        price,
                        size,
                        available_balance
                    );
                }
            }
            Signal::Sell {
                symbol,
                price,
                size,
            } => {
                // 修复：分两步操作，避免借用冲突
                let symbol_string = symbol.to_string();

                // 第一步：获取 actual_size（只读借用）
                let actual_size = positions
                    .get(&symbol_string)
                    .map(|p| size.min(p.size))
                    .unwrap_or(0.0);

                // 第二步：如果需要卖出，再获取可变借用
                if actual_size > 0.0 {
                    let position = positions.get_mut(&symbol_string).unwrap();
                    position.size -= actual_size;

                    let revenue = price * actual_size;
                    available_balance += revenue;

                    // 注意：这里在使用 position 后就计算 equity
                    let should_remove = position.size == 0.0;

                    // 释放 position 的借用后再计算 equity
                    drop(position);

                    let equity = calculate_equity(available_balance, &positions, &candle);
                    equity_curve.push(equity);
                    max_equity = max_equity.max(equity);

                    trades.push(Trade {
                        timestamp: candle.open_timestamp_ms,
                        symbol: symbol.to_string(),
                        side: TradeSide::Sell,
                        price,
                        size: actual_size,
                        balance_after: equity,
                    });

                    tracing::info!(
                        "📉 卖出: {} @ {:.2}, 数量: {:.4}, 余额: {:.2}",
                        symbol,
                        price,
                        actual_size,
                        available_balance
                    );

                    if should_remove {
                        positions.remove(&symbol_string);
                    }
                }
            }
            Signal::Hold => {}
        }
    }

    // 计算最终余额
    let final_balance = available_balance
        + positions
            .values()
            .map(|p| p.size * p.avg_price)
            .sum::<f64>();

    Ok(BacktestReport {
        initial_balance,
        final_balance,
        available_balance,
        positions,
        trades,
        equity_curve,
        max_equity,
    })
}

/// 计算当前总权益
fn calculate_equity(
    available_balance: f64,
    positions: &std::collections::HashMap<String, Position>,
    candle: &CandleData,
) -> f64 {
    let mut equity = available_balance;
    if let Some(position) = positions.get(&candle.symbol.to_string()) {
        equity += position.size * candle.close;
    }
    equity
}

/// 消费订单流
async fn consume_order_stream(
    order_stream: impl Stream<Item = Result<OrderInfo>> + Send,
) -> Result<()> {
    futures::pin_mut!(order_stream);

    while let Some(result) = order_stream.next().await {
        match result {
            Ok(order_info) => {
                println!("✅ 订单执行成功:");
                println!("   订单ID: {}", order_info.ord_id);
                println!("   交易对: {}", order_info.inst_id);
                println!("   客户订单ID: {}", order_info.cl_ord_id);
                println!("{:-<80}", "");
            }
            Err(e) => {
                eprintln!("❌ 订单执行失败: {}", e);
                println!("{:-<80}", "");
            }
        }
    }

    Ok(())
}

// ============== 数据结构 ==============

#[derive(Debug, Clone)]
struct Position {
    size: f64,
    avg_price: f64,
}

#[derive(Debug, Clone)]
struct Trade {
    timestamp: u64,
    symbol: String,
    side: TradeSide,
    price: f64,
    size: f64,
    balance_after: f64,
}

#[derive(Debug, Clone, PartialEq)]
enum TradeSide {
    Buy,
    Sell,
}

#[derive(Debug)]
struct BacktestReport {
    initial_balance: f64,
    final_balance: f64,
    available_balance: f64,
    positions: std::collections::HashMap<String, Position>,
    trades: Vec<Trade>,
    equity_curve: Vec<f64>,
    max_equity: f64,
}

// ============== 报告生成函数 ==============

fn print_backtest_report(report: &BacktestReport) {
    let total_return = report.final_balance - report.initial_balance;
    let total_return_pct = (total_return / report.initial_balance) * 100.0;
    let max_drawdown = calculate_max_drawdown(&report.equity_curve);
    let sharpe_ratio = calculate_sharpe_ratio(&report.equity_curve);
    let (winning_trades, losing_trades) = calculate_win_loss(&report.trades);

    println!("\n{:=<80}", "");
    println!("📊 回测结果摘要");
    println!("{:=<80}", "");
    println!("初始资金: ${:.2}", report.initial_balance);
    println!("最终资金: ${:.2}", report.final_balance);
    println!("可用余额: ${:.2}", report.available_balance);
    println!("总收益: ${:.2}", total_return);
    println!("收益率: {:.2}%", total_return_pct);
    println!("最大回撤: {:.2}%", max_drawdown);
    println!("夏普比率: {:.2}", sharpe_ratio);
    println!("总交易次数: {}", report.trades.len());
    println!("盈利交易: {}", winning_trades);
    println!("亏损交易: {}", losing_trades);

    if winning_trades + losing_trades > 0 {
        let win_rate = winning_trades as f64 / (winning_trades + losing_trades) as f64 * 100.0;
        println!("胜率: {:.2}%", win_rate);
    }

    if !report.positions.is_empty() {
        println!("\n持仓情况:");
        for (symbol, position) in &report.positions {
            if position.size > 0.0 {
                println!(
                    "  {}: {:.4} @ ${:.2}",
                    symbol, position.size, position.avg_price
                );
            }
        }
    }

    println!("{:=<80}\n", "");
}

fn print_trades(trades: &[Trade], limit: Option<usize>) {
    println!("\n交易记录:");
    println!("{:-<100}", "");
    println!(
        "{:<20} {:<15} {:<8} {:<12} {:<10} {:<15}",
        "时间", "交易对", "方向", "价格", "数量", "账户余额"
    );
    println!("{:-<100}", "");

    let trades_to_show = if let Some(n) = limit {
        &trades[..n.min(trades.len())]
    } else {
        trades
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

fn calculate_max_drawdown(equity_curve: &[f64]) -> f64 {
    let mut max_dd: f64 = 0.0;
    let mut peak = equity_curve[0];

    for &equity in equity_curve {
        if equity > peak {
            peak = equity;
        }
        let dd = (peak - equity) / peak * 100.0;
        max_dd = max_dd.max(dd);
    }

    max_dd
}

fn calculate_sharpe_ratio(equity_curve: &[f64]) -> f64 {
    if equity_curve.len() < 2 {
        return 0.0;
    }

    let returns: Vec<f64> = equity_curve
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
        mean_return / std_dev * (252.0_f64).sqrt()
    }
}

fn calculate_win_loss(trades: &[Trade]) -> (usize, usize) {
    use std::collections::HashMap;

    let mut winning = 0;
    let mut losing = 0;
    let mut buy_prices: HashMap<String, Vec<f64>> = HashMap::new();

    for trade in trades {
        match trade.side {
            TradeSide::Buy => {
                buy_prices
                    .entry(trade.symbol.clone())
                    .or_default()
                    .push(trade.price);
            }
            TradeSide::Sell => {
                if let Some(prices) = buy_prices.get_mut(&trade.symbol) {
                    if let Some(buy_price) = prices.pop() {
                        if trade.price > buy_price {
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

