#!/usr/bin/env python3
"""
2026 YTD Turtle+Chandelier Parameter Comparison
Compares 3 Chandelier param sets on 2026 data.
"""

import polars as pl
import numpy as np
from datetime import datetime as dt

SYMBOLS = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"]
PARAMS = [
    ("P=5/M=3.00",  5,  3.00),
    ("P=11/M=2.25", 11, 2.25),
    ("P=15/M=1.50", 15, 1.50),
]
EP = 21
ATR_P = 24
ATR_M = 2.0
HOLD_MAX = 45
TAKER_FEE = 0.001
WARMUP_BARS = 350


def ts_to_dt(ts) -> dt:
    """Convert numpy.datetime64[ms] to Python datetime."""
    return np.datetime64(ts, 'ms').astype('M8[ms]').item()


def true_atr(high, low, bar, period):
    start = max(0, bar - period)
    h_max = max(high[start:bar])
    l_min = min(low[start:bar])
    return h_max - l_min if h_max > l_min else 1.0


def run_symbol(close, high, low, times, cp, cm, min_bar: int, max_bar: int):
    equity = 1.0
    daily_returns = []
    trades = 0
    total_hold = 0.0
    in_pos = False
    bars_held = 0

    for bar in range(min_bar, max_bar + 1):
        if not in_pos:
            ep_start = max(0, bar - EP)
            max_close_prior = max(close[ep_start:bar])
            if close[bar] > max_close_prior:
                in_pos = True
                entry_price = close[bar]
                bars_held = 0
            continue

        bars_held += 1

        atr = true_atr(high, low, bar, ATR_P)
        cp_start = max(0, bar - cp)
        hh_c = max(high[cp_start:bar+1])
        trail_c = hh_c - cm * atr

        atr_start = max(0, bar - ATR_P)
        min_low_atr = min(low[atr_start:bar+1])
        trail_t = min_low_atr - ATR_M * atr

        exit_price = 0.0
        if low[bar] < trail_c:
            exit_price = min(close[bar], trail_c)
        elif low[bar] < trail_t:
            exit_price = min(close[bar], trail_t)
        elif bars_held >= HOLD_MAX:
            exit_price = close[bar]

        if exit_price > 0:
            ret = (exit_price / entry_price - 1.0) - TAKER_FEE
            equity *= (1.0 + ret)
            daily_returns.append(ret)
            total_hold += bars_held
            trades += 1
            in_pos = False

    avg_hold = total_hold / trades if trades > 0 else 0.0
    return equity, daily_returns, trades, avg_hold


def main():
    start_date = dt(2026, 1, 1)
    end_date = dt(2026, 4, 20)
    # Convert to numpy datetime64 for filtering
    start_np = np.datetime64(start_date, 'ms')
    end_np = np.datetime64(end_date, 'ms')

    # Load data
    data = {}
    for sym in SYMBOLS:
        path = f"data/cache/{sym.lower()}_1d.parquet"
        df = pl.read_parquet(path)
        # Filter to end date
        df = df.filter(pl.col("time") <= end_np)
        times = df["time"].to_numpy()
        # Find first index >= start_date
        start_idx = None
        for i, t in enumerate(times):
            if t >= start_np:
                start_idx = i
                break
        if start_idx is None or start_idx == 0:
            print(f"Skipped {sym}: no data after {start_date}")
            continue
        load_start = max(0, start_idx - WARMUP_BARS)
        df_s = df[load_start:]
        data[sym] = {
            "close": df_s["close"].to_numpy(),
            "high": df_s["high"].to_numpy(),
            "low": df_s["low"].to_numpy(),
            "times": df_s["time"].to_numpy(),
            "start_2026_idx": start_idx - load_start,  # index within this slice
            "end_idx": len(df_s) - 1,
        }
        print(f"Loaded {sym}: {len(df_s)} rows (warmup={load_start}, 2026 start idx={start_idx - load_start})")

    # BTC buy-hold reference
    if "BTCUSDT" in data:
        d = data["BTCUSDT"]
        si = d["start_2026_idx"]
        ei = d["end_idx"]
        btc_ret = d["close"][ei] / d["close"][si] - 1.0
        n_days = ei - si + 1
        print(f"\nBTC buy-hold 2026 YTD: {btc_ret:+.1%} ({n_days} trading days)")
    print()

    print(f"{'Param':<16} {'Portfolio':>10} {'Sharpe':>7} {'Trades':>6} | Per-symbol returns")
    print("-" * 90)

    for label, cp, cm in PARAMS:
        results = {}
        all_returns = []

        for sym in SYMBOLS:
            if sym not in data:
                continue
            d = data[sym]
            min_bar = d["start_2026_idx"]
            max_bar = d["end_idx"]
            eq, rets, trades, avg_hold = run_symbol(
                d["close"], d["high"], d["low"], d["times"], cp, cm, min_bar, max_bar
            )
            results[sym] = {"equity": eq, "trades": trades, "avg_hold": avg_hold}
            all_returns.extend(rets)

        if not results:
            print(f"{label}: no data")
            continue

        n = len(results)
        port_eq = 1.0
        for r in results.values():
            port_eq *= r["equity"] ** (1.0 / n)
        total_trades = sum(r["trades"] for r in results.values())

        if len(all_returns) > 5:
            mean_r = sum(all_returns) / len(all_returns)
            std_r = (sum((x - mean_r)**2 for x in all_returns) / len(all_returns)) ** 0.5
            sharpe = (mean_r / std_r * (252**0.5)) if std_r > 0 else 0.0
        else:
            sharpe = 0.0

        sym_parts = [f"{sym[:3]}={results[sym]['equity']-1:+.0%}({results[sym]['trades']})" for sym in SYMBOLS if sym in results]
        print(f"{label:<16} {port_eq-1:>+10.1%} {sharpe:>7.2f} {total_trades:>6} | {'  '.join(sym_parts)}")

    print()
    print("Note: Equal-weight geometric mean portfolio across 6 symbols. Warmup bars provide lookback history.")


if __name__ == "__main__":
    main()
