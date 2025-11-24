"""
使用 CCXT 库从多个交易所获取历史K线数据并保存为 CSV 格式
支持多交易所、多交易对、多时间周期
"""

import argparse
import os
import time
from datetime import datetime, timedelta
from typing import List, Optional

import ccxt
import polars as pl
from tqdm import tqdm

# 时间周期映射到秒数
TIMEFRAME_TO_SECONDS = {
    "1m": 60,
    "3m": 180,
    "5m": 300,
    "15m": 900,
    "30m": 1800,
    "1h": 3600,
    "2h": 7200,
    "4h": 14400,
    "6h": 21600,
    "12h": 43200,
    "1d": 86400,
    "3d": 259200,
    "1w": 604800,
}


def get_exchange(exchange_name: str, proxy_url: Optional[str] = None):
    """
    创建交易所实例，支持代理配置
    """
    exchange_classes = {
        'binance': ccxt.binance,
        'okx': ccxt.okx,
        'bybit': ccxt.bybit,
        'huobi': ccxt.huobi,
        'kraken': ccxt.kraken,
        'coinbase': ccxt.coinbase,
    }
    
    if exchange_name.lower() not in exchange_classes:
        raise ValueError(f"不支持的交易所: {exchange_name}. 支持的交易所: {list(exchange_classes.keys())}")
    
    exchange_class = exchange_classes[exchange_name.lower()]
    
    # 基础配置
    config = {
        'enableRateLimit': True,  # 启用速率限制
        'options': {
            'defaultType': 'spot',  # 现货市场
        }
    }

    # 如果提供了代理，添加到配置中
    if proxy_url:
        config['proxies'] = {
            'http': proxy_url,
            'https': proxy_url,
        }
        # 部分交易所可能需要显式设置 aiohttp_proxy (如果未来切换到异步)
        # 或者是 specific proxy settings
    
    exchange = exchange_class(config)
    
    return exchange


def convert_symbol_to_csv_format(symbol: str) -> str:
    """
    转换交易对格式: BTC/USDT -> BTC-USDT
    """
    return symbol.replace("/", "-")


def convert_csv_to_ccxt_format(symbol: str) -> str:
    """
    转换交易对格式: BTC-USDT -> BTC/USDT
    """
    return symbol.replace("-", "/")


def fetch_ohlcv_data(
    exchange,
    symbol: str,
    timeframe: str,
    start_time: int,
    end_time: int,
    max_retries: int = 3,
) -> List[List]:
    """
    获取 OHLCV 数据

    Args:
        exchange: CCXT 交易所实例
        symbol: 交易对，如 'BTC/USDT'
        timeframe: 时间周期，如 '1m', '5m', '1h'
        start_time: 开始时间（毫秒时间戳）
        end_time: 结束时间（毫秒时间戳）
        max_retries: 最大重试次数

    Returns:
        List of [timestamp, open, high, low, close, volume]
    """
    all_data = []
    current_start = start_time

    # 计算时间间隔（毫秒）
    timeframe_ms = TIMEFRAME_TO_SECONDS[timeframe] * 1000
    limit = 1000  # 大多数交易所每次最多返回 1000 条

    # 创建进度条
    total_candles = int((end_time - start_time) / timeframe_ms)
    pbar = tqdm(total=total_candles, desc=f"获取 {symbol} {timeframe} 数据")


    while current_start < end_time:
        retry_count = 0
        success = False

        while retry_count < max_retries and not success:
            try:
                # 获取数据
                ohlcv = exchange.fetch_ohlcv(
                    symbol=symbol, timeframe=timeframe, since=current_start, limit=limit
                )

                if not ohlcv:
                    break

                # 过滤掉超出结束时间的数据
                filtered_ohlcv = [candle for candle in ohlcv if candle[0] < end_time]

                if not filtered_ohlcv:
                    break

                all_data.extend(filtered_ohlcv)
                pbar.update(len(filtered_ohlcv))

                # 更新下一次请求的起始时间
                current_start = filtered_ohlcv[-1][0] + timeframe_ms

                success = True

                # 速率限制
                if exchange.rateLimit:
                    time.sleep(exchange.rateLimit / 1000)

            except ccxt.NetworkError as e:
                retry_count += 1
                print(f"\n网络错误 ({retry_count}/{max_retries}): {e}")
                time.sleep(2**retry_count)  # 指数退避

            except ccxt.ExchangeError as e:
                retry_count += 1
                print(f"\n交易所错误 ({retry_count}/{max_retries}): {e}")
                time.sleep(2**retry_count)

            except Exception as e:
                print(f"\n未知错误: {e}")
                break

        if not success:
            print(f"\n获取数据失败，已重试 {max_retries} 次")
            break

    pbar.close()

    # 去重（基于时间戳）
    unique_data = {}
    for candle in all_data:
        timestamp = candle[0]
        if timestamp not in unique_data:
            unique_data[timestamp] = candle

    # 按时间戳排序
    sorted_data = sorted(unique_data.values(), key=lambda x: x[0])

    return sorted_data


def save_to_csv(
    data: List[List],
    exchange_name: str,
    symbol: str,
    timeframe: str,
    output_dir: str = "../data",
) -> str:
    """
    将数据保存为 CSV 格式 (使用 Polars)

    CSV 格式：symbol,interval_sc,open_timestamp_ms,open,high,low,close,volume
    """
    if not data:
        print("没有数据可保存！")
        return None

    # 转换交易对格式
    csv_symbol = convert_symbol_to_csv_format(symbol)

    # 获取时间间隔（秒）
    interval_sc = TIMEFRAME_TO_SECONDS[timeframe]

    # 构建数据
    df_data = []
    for candle in data:
        df_data.append(
            {
                "symbol": csv_symbol,
                "interval_sc": interval_sc,
                "open_timestamp_ms": int(candle[0]),
                "open": float(candle[1]),
                "high": float(candle[2]),
                "low": float(candle[3]),
                "close": float(candle[4]),
                "volume": float(candle[5]),
            }
        )

    # 使用 Polars 创建 DataFrame
    df = pl.DataFrame(df_data)

    # 确保目录存在
    os.makedirs(output_dir, exist_ok=True)

    # 构建文件名
    filename = f"{exchange_name}_{csv_symbol.lower().replace('/', '-')}_{timeframe}.csv"
    output_path = os.path.join(output_dir, filename)

    # 保存到 CSV (Polars 不需要指定 index=False，这是默认行为)
    df.write_csv(output_path)

    print(f"\n数据已保存到: {output_path}")
    print(f"总条数: {len(df)}")
    print(
        f"时间范围: {datetime.fromtimestamp(df['open_timestamp_ms'].min()/1000)} 到 {datetime.fromtimestamp(df['open_timestamp_ms'].max()/1000)}"
    )
    print(f"价格范围: {df['close'].min():.2f} - {df['close'].max():.2f}")

    return output_path


def parse_date(date_str: str) -> datetime:
    """
    解析日期字符串
    支持格式: YYYY-MM-DD 或 YYYY-MM-DD HH:MM:SS
    """
    formats = ["%Y-%m-%d", "%Y-%m-%d %H:%M:%S"]
    for fmt in formats:
        try:
            return datetime.strptime(date_str, fmt)
        except ValueError:
            continue
    raise ValueError(
        f"无法解析日期: {date_str}. 支持的格式: YYYY-MM-DD 或 YYYY-MM-DD HH:MM:SS"
    )


def load_existing_data(filepath: str) -> Optional[pl.DataFrame]:
    """
    加载已存在的数据文件 (使用 Polars)
    """
    if os.path.exists(filepath):
        try:
            df = pl.read_csv(filepath)
            return df
        except Exception as e:
            print(f"读取现有文件失败: {e}")
    return None


def merge_and_save_data(
    new_data: List[List],
    exchange_name: str,
    symbol: str,
    timeframe: str,
    output_dir: str = "../data",
    incremental: bool = False,
) -> str:
    """
    合并新旧数据并保存（支持增量更新，使用 Polars）
    """
    csv_symbol = convert_symbol_to_csv_format(symbol)
    filename = f"{exchange_name}_{csv_symbol.lower().replace('/', '-')}_{timeframe}.csv"
    output_path = os.path.join(output_dir, filename)

    if incremental and os.path.exists(output_path):
        print(f"\n检测到已存在的数据文件，进行增量更新...")
        existing_df = load_existing_data(output_path)

        if existing_df is not None and len(existing_df) > 0:
            # 转换新数据为 DataFrame
            interval_sc = TIMEFRAME_TO_SECONDS[timeframe]
            new_df_data = []
            for candle in new_data:
                new_df_data.append(
                    {
                        "symbol": csv_symbol,
                        "interval_sc": interval_sc,
                        "open_timestamp_ms": int(candle[0]),
                        "open": float(candle[1]),
                        "high": float(candle[2]),
                        "low": float(candle[3]),
                        "close": float(candle[4]),
                        "volume": float(candle[5]),
                    }
                )
            new_df = pl.DataFrame(new_df_data)

            # 合并数据 (使用 Polars concat)
            # Polars 要求列类型一致，这里数据源构建方式相同，类型应是一致的
            combined_df = pl.concat([existing_df, new_df])

            # 去重并排序
            # Polars 使用 unique，keep='last' 保留最后出现的重复项
            combined_df = combined_df.unique(subset=["open_timestamp_ms"], keep="last")
            # Polars 使用 sort
            combined_df = combined_df.sort("open_timestamp_ms")

            # 保存
            os.makedirs(output_dir, exist_ok=True)
            combined_df.write_csv(output_path)

            print(f"数据已更新到: {output_path}")
            print(f"原有数据: {len(existing_df)} 条")
            print(f"新增数据: {len(new_df)} 条")
            print(f"合并后总计: {len(combined_df)} 条")

            return output_path

    # 如果不是增量更新或文件不存在，直接保存
    return save_to_csv(new_data, exchange_name, symbol, timeframe, output_dir)


def main():
    parser = argparse.ArgumentParser(
        description="使用 CCXT 从加密货币交易所获取历史K线数据",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
示例用法:
  # 获取 Binance 的 BTC/USDT 最近 7 天的 1 分钟数据
  python fetch_data_ccxt.py
  
  # 指定交易所和交易对
  python fetch_data_ccxt.py --exchange binance --symbol BTC/USDT --days 7
  
  # 指定时间周期
  python fetch_data_ccxt.py --exchange binance --symbol BTC/USDT --timeframe 5m --days 30
  
  # 多个交易对
  python fetch_data_ccxt.py --exchange binance --symbols BTC/USDT ETH/USDT --days 7
  
  # 指定具体的开始和结束日期
  python fetch_data_ccxt.py --exchange binance --symbol BTC/USDT --start 2025-01-01 --end 2025-01-31
  
  # 增量更新模式
  python fetch_data_ccxt.py --exchange binance --symbol BTC/USDT --incremental
        """,
    )

    parser.add_argument(
        "--exchange",
        type=str,
        default="binance",
        help="交易所名称 (binance, okx, bybit, huobi, kraken, coinbase)",
    )
    parser.add_argument(
        "--symbol", type=str, default="BTC/USDT", help="交易对，如 BTC/USDT"
    )
    parser.add_argument(
        "--symbols", nargs="+", type=str, help="多个交易对，如 BTC/USDT ETH/USDT"
    )
    parser.add_argument(
        "--timeframe",
        type=str,
        default="1m",
        choices=list(TIMEFRAME_TO_SECONDS.keys()),
        help="时间周期",
    )
    parser.add_argument("--days", type=int, default=7, help="获取最近多少天的数据")
    parser.add_argument(
        "--start", type=str, help="开始日期 (格式: YYYY-MM-DD 或 YYYY-MM-DD HH:MM:SS)"
    )
    parser.add_argument(
        "--end", type=str, help="结束日期 (格式: YYYY-MM-DD 或 YYYY-MM-DD HH:MM:SS)"
    )
    parser.add_argument("--output", type=str, default="../data", help="输出目录路径")
    parser.add_argument(
        "--incremental", action="store_true", help="增量更新模式（合并已有数据）"
    )
    # 代理参数 (默认使用你提供的地址)
    parser.add_argument('--proxy-host', type=str, default='127.0.0.1', help='代理主机 IP')
    parser.add_argument('--proxy-port', type=int, default=7890, help='代理端口')

    args = parser.parse_args()

    # 构建代理 URL
    proxy_url = None
    if args.proxy_host and args.proxy_port:
        proxy_url = f"http://{args.proxy_host}:{args.proxy_port}"

    # 确定交易对列表
    symbols = args.symbols if args.symbols else [args.symbol]

    # 确定时间范围
    if args.start and args.end:
        start_dt = parse_date(args.start)
        end_dt = parse_date(args.end)
    elif args.start:
        start_dt = parse_date(args.start)
        end_dt = datetime.now()
    elif args.end:
        end_dt = parse_date(args.end)
        start_dt = end_dt - timedelta(days=args.days)
    else:
        end_dt = datetime.now()
        start_dt = end_dt - timedelta(days=args.days)

    start_time = int(start_dt.timestamp() * 1000)
    end_time = int(end_dt.timestamp() * 1000)

    print(f"\n{'='*60}")
    print(f"开始获取数据")
    print(f"{'='*60}")
    print(f"交易所: {args.exchange}")
    print(f"代理地址: {proxy_url if proxy_url else '未使用代理'}")
    print(f"交易对: {', '.join(symbols)}")
    print(f"时间周期: {args.timeframe}")
    print(f"时间范围: {start_dt} 到 {end_dt}")
    print(f"增量更新: {'是' if args.incremental else '否'}")
    print(f"{'='*60}\n")

    try:
        # 创建交易所实例
        exchange = get_exchange(args.exchange, proxy_url)

        # 遍历所有交易对
        for symbol in symbols:
            print(f"\n处理交易对: {symbol}")
            print("-" * 60)

            try:
                # 获取数据
                ohlcv_data = fetch_ohlcv_data(
                    exchange=exchange,
                    symbol=symbol,
                    timeframe=args.timeframe,
                    start_time=start_time,
                    end_time=end_time,
                )

                if not ohlcv_data:
                    print(f"未获取到 {symbol} 的数据")
                    continue

                # 保存数据
                merge_and_save_data(
                    new_data=ohlcv_data,
                    exchange_name=args.exchange,
                    symbol=symbol,
                    timeframe=args.timeframe,
                    output_dir=args.output,
                    incremental=args.incremental,
                )

            except Exception as e:
                print(f"处理 {symbol} 时出错: {e}")
                continue

        print(f"\n{'='*60}")
        print("所有数据获取完成！")
        print(f"{'='*60}\n")

    except Exception as e:
        print(f"\n错误: {e}")
        return 1

    return 0


if __name__ == "__main__":
    exit(main())
