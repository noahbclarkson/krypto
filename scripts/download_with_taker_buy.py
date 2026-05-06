#!/usr/bin/env python3
"""
Download historical klines for Base5 symbols with taker-buy-volume fields.
Binance klines (candlesticks) include:
  - taker_buy_quote_asset_volume: volume of taker buy orders (in quote currency)
  - quote_asset_volume: total volume (in quote currency)

Taker buy pressure = taker_buy_quote_vol / quote_vol
Values > 0.5 = net buyer-initiated (bullish pressure)
Values < 0.5 = net seller-initiated (bearish pressure)

Usage:
  python3 scripts/download_with_taker_buy.py [--symbol BTCUSDT] [--days 2000]
"""

import argparse
import json
import time
import requests
from datetime import datetime, timezone
from pathlib import Path

BASE_URL = "https://api.binance.com"
CACHE_DIR = Path("data/cache/taker_buy")
CACHE_DIR.mkdir(parents=True, exist_ok=True)

HEADERS = {"User-Agent": "krypto-taker-buy-downloader/1.0"}

def fetch_klines_with_taker_buy(symbol: str, interval: str = "1d",
                                 start_time: int = None, end_time: int = None,
                                 limit: int = 1000) -> list[dict]:
    """Fetch klines including taker buy volume fields."""
    params = {"symbol": symbol, "interval": interval, "limit": limit}
    if start_time:
        params["startTime"] = start_time
    if end_time:
        params["endTime"] = end_time
    
    resp = requests.get(f"{BASE_URL}/api/v3/klines", params=params, 
                        headers=HEADERS, timeout=10)
    resp.raise_for_status()
    raw = resp.json()
    
    result = []
    for k in raw:
        # Binance kline fields (12 total):
        # [open_time, open, high, low, close, volume, close_time,
        #  quote_vol, num_trades, taker_buy_base_vol, taker_buy_quote_vol, ignore]
        result.append({
            "open_time": k[0],
            "open": float(k[1]),
            "high": float(k[2]),
            "low": float(k[3]),
            "close": float(k[4]),
            "volume": float(k[5]),
            "close_time": k[6],
            "quote_vol": float(k[7]),
            "num_trades": int(k[8]),
            "taker_buy_base_vol": float(k[9]),
            "taker_buy_quote_vol": float(k[10]),
        })
    return result


def download_symbol(symbol: str, interval: str = "1d", 
                     days_back: int = 2000) -> list[dict]:
    """Download full history for a symbol in paginated chunks."""
    now_ms = int(time.time() * 1000)
    start_ms = now_ms - days_back * 24 * 3600 * 1000
    
    all_klines = []
    current_start = start_ms
    
    while True:
        batch = fetch_klines_with_taker_buy(symbol, interval, 
                                           start_time=current_start,
                                           limit=1000)
        if not batch:
            break
        
        all_klines.extend(batch)
        last_time = batch[-1]["open_time"]
        
        # Move start past this batch (add 1ms to avoid overlap)
        current_start = last_time + 1
        
        if len(batch) < 1000:
            break
        
        # Safety: limit total batches to avoid runaway
        if len(all_klines) > 50000:
            print(f"  WARNING: hit 50000 bar cap for {symbol}, truncating")
            break
        
        time.sleep(0.2)  # be polite
    
    # Sort and dedupe by open_time
    all_klines.sort(key=lambda k: k["open_time"])
    seen = set()
    unique = []
    for k in all_klines:
        if k["open_time"] not in seen:
            seen.add(k["open_time"])
            unique.append(k)
    
    print(f"  {symbol}: {len(unique)} unique bars downloaded")
    return unique


def compute_features(klines: list[dict]) -> list[dict]:
    """Add taker_buy_pressure ratio and EMA smoothing."""
    for k in klines:
        qv = k["quote_vol"]
        tbqv = k["taker_buy_quote_vol"]
        k["taker_buy_pressure"] = tbqv / qv if qv > 0 else 0.5
    return klines


def save_parquet(klines: list[dict], symbol: str, interval: str = "1d"):
    """Save to cache as JSON (simple) + compute features."""
    import polars as pl
    
    rows = compute_features(klines)
    
    df = pl.DataFrame({
        "time_ms": [r["open_time"] for r in rows],
        "time": [datetime.fromtimestamp(r["open_time"]/1000, tz=timezone.utc) 
                 for r in rows],
        "open": [r["open"] for r in rows],
        "high": [r["high"] for r in rows],
        "low": [r["low"] for r in rows],
        "close": [r["close"] for r in rows],
        "volume": [r["volume"] for r in rows],
        "quote_vol": [r["quote_vol"] for r in rows],
        "num_trades": [r["num_trades"] for r in rows],
        "taker_buy_base_vol": [r["taker_buy_base_vol"] for r in rows],
        "taker_buy_quote_vol": [r["taker_buy_quote_vol"] for r in rows],
        "taker_buy_pressure": [r["taker_buy_pressure"] for r in rows],
    })
    
    path = CACHE_DIR / f"{symbol.lower()}_{interval}.parquet"
    df.write_parquet(str(path))
    print(f"  Saved {len(df)} rows → {path}")
    return path


def main():
    parser = argparse.ArgumentParser(description="Download klines with taker buy volume")
    parser.add_argument("--symbol", default=None, help="Single symbol (e.g. BTCUSDT)")
    parser.add_argument("--days", type=int, default=2000, help="Days back to fetch")
    parser.add_argument("--interval", default="1d")
    args = parser.parse_args()
    
    symbols = [args.symbol.upper()] if args.symbol else ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"]
    
    for sym in symbols:
        print(f"\nDownloading {sym} ({args.interval}, {args.days}d back)...")
        klines = download_symbol(sym, interval=args.interval, days_back=args.days)
        if klines:
            save_parquet(klines, sym, interval=args.interval)
            # Show sample
            k = klines[-1]
            dt = datetime.fromtimestamp(k["open_time"]/1000, tz=timezone.utc)
            print(f"  Latest: {dt.strftime('%Y-%m-%d')} close={k['close']:.4f} "
                  f"pressure={k['taker_buy_quote_vol']/k['quote_vol']:.3f}")
        time.sleep(0.3)
    
    print(f"\nDone. Cache: {CACHE_DIR}")


if __name__ == "__main__":
    main()