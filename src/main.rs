mod backtest;

use backtest::{BacktestEngine, BacktestResult};
use ephemera_source::csv::csv_candle_data_stream;
use ephemera_strategy::{
    StrategyContext,
    strategies::{MACrossStrategy, Strategy},
};
use futures::StreamExt;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("开始运行回测...\n");

    // 配置参数
    let data_path = "data/binance_btc-usdt_1m.csv";
    let symbol = "BTC-USDT";
    let initial_balance = 10000.0;
    let position_size = 0.01; // 每次交易 0.01 BTC

    // 创建策略：双均线交叉策略 (快线5, 慢线10)
    let mut strategy = MACrossStrategy::new(symbol.into(), 5, 10, position_size);

    // 创建回测引擎
    let mut backtest = BacktestEngine::new(initial_balance);

    // 读取数据流
    let mut stream = csv_candle_data_stream(data_path).await?;

    let mut candle_count = 0;
    let mut last_candle = None;

    println!("正在处理K线数据...");

    while let Some(result) = stream.next().await {
        match result {
            Ok(candle) => {
                candle_count += 1;

                // 策略生成信号
                if let Ok(Some(signal)) = strategy.on_data(candle.clone()).await {
                    if !signal.is_hold() {
                        // 处理交易信号
                        backtest.process_signal(signal, &candle);
                    }
                }

                last_candle = Some(candle);

                // 每1000条数据打印进度
                if candle_count % 1000 == 0 {
                    println!("已处理 {} 条K线数据...", candle_count);
                }
            }
            Err(e) => {
                eprintln!("读取K线数据错误: {}", e);
                break;
            }
        }
    }

    println!("数据处理完成，共处理 {} 条K线\n", candle_count);

    // 生成并打印回测报告
    if let Some(last) = last_candle {
        let result = backtest.generate_report(&last);
        result.print_summary();
        result.print_trades(Some(10)); // 只显示前10条交易
    } else {
        println!("没有数据可供回测！");
    }

    Ok(())
}
