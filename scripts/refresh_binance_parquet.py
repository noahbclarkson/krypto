#!/usr/bin/env python3
"""Refresh Binance 1d parquet cache files with latest candles from public API."""
import requests
import polars as pl
import datetime as dt
import time

API = "https://api.binance.com/api/v3/klines"
CACHE = "data/cache"
DAYS = 10

syms = ["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT","LTCUSDT","EOSUSDT","BNBUSDT","BCHUSDT"]

for sym in syms:
    path = f"{CACHE}/{sym.lower()}_1d.parquet"
    try:
        ex = pl.read_parquet(path)
        # Store existing timestamps as i64 ms for fast deduplication (avoids precision mismatch)
        ex_ts_ms = set(int(t.timestamp() * 1000) for t in ex["time"].to_list())
        print(f"{sym}: {len(ex)} rows, last {ex[-1,'time']}")
    except Exception as e:
        ex = pl.DataFrame()
        ex_ts_ms = set()
        print(f"{sym}: no cache ({e})")

    params = {"symbol": sym, "interval": "1d", "limit": DAYS}
    r = requests.get(API, params=params, timeout=15)
    r.raise_for_status()

    times_ms = [int(k[0]) for k in r.json()]
    opens   = [float(k[1]) for k in r.json()]
    highs   = [float(k[2]) for k in r.json()]
    lows    = [float(k[3]) for k in r.json()]
    closes  = [float(k[4]) for k in r.json()]
    volumes = [float(k[5]) for k in r.json()]

    # Build DataFrame: store as i64 ms timestamps, then cast to datetime(ms)
    new = pl.DataFrame({
        "time": pl.Series("time", times_ms, dtype=pl.Int64),
        "open": opens, "high": highs, "low": lows,
        "close": closes, "volume": volumes,
    }).with_columns(
        pl.col("time").cast(pl.Datetime("ms"))
    )

    # Filter using ms timestamps to avoid datetime precision issues
    new_times_ms = [int(t.timestamp() * 1000) for t in new["time"].to_list()]
    new_only = new.filter(
        pl.Series([t not in ex_ts_ms for t in new_times_ms])
    )

    if len(new_only) == 0:
        print(f"  -> cache current")
    else:
        comb = pl.concat([ex, new_only]).sort("time")
        comb.write_parquet(path)
        print(f"  -> +{len(new_only)} new rows, total {len(comb)}, last {comb[-1,'time']}")
    time.sleep(0.3)
print("DONE")
