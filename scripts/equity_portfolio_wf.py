#!/usr/bin/env python3
"""
Equity Portfolio Walk-Forward: SPY/QQQ/GLD + BTC/ETH/SOL combined Turtle portfolio.

Tests whether adding lower-volatility equities (SPY/QQQ/GLD) to the Turtle+Chandelier
portfolio reduces MaxDD without proportionally reducing returns.

Hypothesis: Equities negatively correlate with crypto in stress events (2022: SPY -19% vs BTC -64%).
Combined portfolio should have lower MaxDD than crypto-only Turtle.

FROZEN crypto params: EP=21, CHAND(28,2.15), ATR(24,2.0), HM=45, CAP=3
Walk-forward: 252-bar train / 252-bar test
"""
import polars as pl
import numpy as np
from datetime import datetime

# ── Config ───────────────────────────────────────────────────────
TRAIN, TEST = 252, 252
TAKER_FEE = 0.001
MIN_TRADES = 3
HOLD_MAX = 45
EP = 21
CHAND_P, CHAND_M = 28, 2.15
ATR_P, ATR_M = 24, 2.00
CAP = 3

CRYPTO = ["BTCUSDT", "ETHUSDT", "SOLUSDT"]
EQUITY = ["SPY", "QQQ", "GLD"]
ALL_ASSETS = CRYPTO + EQUITY

DATA_DIR = "data/cache"

# ── Load data ────────────────────────────────────────────────────
def load_crypto(ticker: str) -> pl.DataFrame:
    path = f"{DATA_DIR}/{ticker.lower()}_1d.parquet"
    df = pl.read_parquet(path)
    # Ensure time column is parsed
    if "time" in df.columns:
        df = df.with_columns(pl.col("time").str.to_datetime("%Y-%m-%d %H:%M:%S"))
    date_col = "time"
    # Use only the columns we need, renamed to lowercase
    return df.select([
        pl.col(date_col).cast(pl.Date).alias("date"),
        pl.col("close").alias("close"),
        pl.col("high").alias("high"),
        pl.col("low").alias("low"),
        pl.col("volume").alias("vol"),
    ])

def load_equity(ticker: str) -> pl.DataFrame:
    path = f"{DATA_DIR}/{ticker.lower()}_1d_equity.parquet"
    df = pl.read_parquet(path)
    df = df.with_columns(pl.col("date").str.to_datetime("%Y-%m-%d"))
    return df.select([
        pl.col("date"),
        pl.col("close"),
        pl.col("high"),
        pl.col("low"),
        pl.col("volume").alias("vol"),
    ])

print("Loading data...")
crypto_dfs = {t: load_crypto(t) for t in CRYPTO}
equity_dfs = {t: load_equity(t) for t in EQUITY}

# Find common date range (crypto drives the start)
crypto_start = max(df["date"].min() for df in crypto_dfs.values())
crypto_end   = min(df["date"].max() for df in crypto_dfs.values())
equity_start = max(df["date"].min() for df in equity_dfs.values())
equity_end   = min(df["date"].max() for df in equity_dfs.values())

# Common range = intersection
common_start = max(crypto_start, equity_start)
common_end   = min(crypto_end,   equity_end)
print(f"Common date range: {common_start} → {common_end}")

# Build aligned daily bars for each asset
def align_df(df: pl.DataFrame, start, end) -> pl.DataFrame:
    df = df.filter(pl.col("date").is_between(start, end, closed="both"))
    return df.sort("date")

aligned = {}
for t in CRYPTO:
    aligned[t] = align_df(crypto_dfs[t], common_start, common_end).with_row_index("bar")
for t in EQUITY:
    aligned[t] = align_df(equity_dfs[t], common_start, common_end).with_row_index("bar")

# Verify all same length
bars_per_asset = {t: aligned[t].height for t in ALL_ASSETS}
print(f"Bars per asset: {bars_per_asset}")
min_bars = min(bars_per_asset.values())
for t in ALL_ASSETS:
    aligned[t] = aligned[t].head(min_bars)

n = min_bars
total_windows = (n - TRAIN - TEST) // TEST
print(f"Total bars: {n}, Walk-forward windows: {total_windows}\n")

# ── ATR helper ──────────────────────────────────────────────────
def atr(highs, lows, closes, period, idx):
    if idx < period:
        return np.nan
    trs = []
    for i in range(idx + 1 - period, idx + 1):
        h, l, c0 = highs[i], lows[i], closes[i-1] if i > 0 else closes[0]
        tr = max(h - l, abs(h - c0), abs(l - c0))
        trs.append(tr)
    return np.mean(trs)

# ── Walk-forward ───────────────────────────────────────────────
records = []
for wi in range(total_windows):
    train_end  = TRAIN + wi * TEST
    test_start = train_end
    test_end   = min(test_start + TEST, n)
    actual_test = test_end - test_start
    if actual_test < 30:
        continue

    # Build test arrays for each asset
    close = {t: aligned[t]["close"].to_numpy() for t in ALL_ASSETS}
    high  = {t: aligned[t]["high"].to_numpy()  for t in ALL_ASSETS}
    low   = {t: aligned[t]["low"].to_numpy()   for t in ALL_ASSETS}
    vol   = {t: aligned[t]["vol"].to_numpy()   for t in ALL_ASSETS}

    equity = 1.0
    peak = 1.0
    wins = 0
    total_trades = 0
    daily_rets = []

    bar = test_start
    while bar + 2 < test_end:
        # Dollar-volume ranking at this bar
        scores = []
        for t in ALL_ASSETS:
            if bar >= len(close[t]) or bar >= len(vol[t]):
                continue
            dv = vol[t][bar] * close[t][bar] if vol[t][bar] > 0 and close[t][bar] > 0 else 0.0
            scores.append((dv, t))
        scores.sort(reverse=True)
        top_syms = [t for _, t in scores[:CAP]]

        entered = False
        for sym in top_syms:
            c_arr = close[sym]
            h_arr = high[sym]
            l_arr = low[sym]
            if bar < EP + 1 or bar >= len(c_arr):
                continue

            # Turtle entry: close > max(close[bar-EP:bar])
            max_close = max(c_arr[bar - EP : bar]) if bar >= EP else c_arr[bar]
            if c_arr[bar] <= max_close:
                continue

            # Entry price (taker fee)
            entry_px = c_arr[bar]
            entry = entry_px * (1.0 - TAKER_FEE)
            entry_bar_next = bar + 1
            n_bars = len(c_arr)

            # Dual exit: Chandelier OR Turtle ATR fires first
            highest_chand = h_arr[entry_bar_next]
            highest_turtle = h_arr[entry_bar_next]
            max_bar = min(entry_bar_next + HOLD_MAX, n_bars - 1)
            exit_bar = max_bar

            for b in range(entry_bar_next, max_bar + 1):
                highest_chand = max(highest_chand, h_arr[b])
                highest_turtle = max(highest_turtle, h_arr[b])
                
                atr_c = atr(h_arr, l_arr, c_arr, CHAND_P, b)
                trail_chand = highest_chand - CHAND_M * atr_c
                
                atr_t = atr(h_arr, l_arr, c_arr, ATR_P, b)
                trail_turtle = highest_turtle - ATR_M * atr_t
                
                if c_arr[b] < trail_chand or c_arr[b] < trail_turtle:
                    exit_bar = b
                    break

            if exit_bar < len(c_arr):
                exit_px = c_arr[exit_bar]
                exit = exit_px * (1.0 - TAKER_FEE)
                gross_ret = exit / entry - 1.0
                bars_held = max(exit_bar - entry_bar_next, 1)
                
                wins += 1 if gross_ret > 0 else 0
                total_trades += 1
                equity *= (1.0 + gross_ret)
                peak = max(peak, equity)
                
                avg_daily = gross_ret / bars_held
                for _ in range(bars_held):
                    daily_rets.append(avg_daily)
                
                bar = exit_bar + 1
                entered = True
                break

        if not entered:
            bar += 1

    # Compute metrics
    ret = (equity - 1.0) * 100.0
    if len(daily_rets) >= 2:
        mn = np.mean(daily_rets)
        sd = np.std(daily_rets, ddof=0)
        sharpe = mn / sd * np.sqrt(365) if sd > 0 else 0.0
    else:
        sharpe = 0.0
    
    equity_arr = [1.0]
    peak_check = 1.0
    max_dd = 0.0
    # Rebuild equity curve for DD calc
    # (already tracked inline above)
    # Use equity as final value
    mdd = (peak - equity) / peak * 100.0
    win_rate = wins / total_trades * 100.0 if total_trades > 0 else 0.0
    passed = total_trades >= MIN_TRADES and ret > 0.0

    records.append({
        "window": wi,
        "return_pct": ret,
        "sharpe": sharpe,
        "max_dd_pct": mdd,
        "trades": total_trades,
        "win_rate_pct": win_rate,
        "pass": passed,
    })
    print(f"  W{wi:02d} | {ret:+8.1f}% sh={sharpe:6.2f} DD={mdd:5.1f}% "
          f"{total_trades:4d}t {win_rate:3.0f}% {'PASS' if passed else 'FAIL'}")

# Summary
df_results = pl.DataFrame(records)
n_win = len(df_results)
n_pass = df_results["pass"].sum()
avg_ret = df_results["return_pct"].mean()
avg_sh  = df_results["sharpe"].mean()
worst_dd = df_results["max_dd_pct"].max()
total_trades = df_results["trades"].sum()

print(f"\n=== COMBINED PORTFOLIO WALK-FORWARD ===")
print(f"Assets: {ALL_ASSETS}")
print(f"Windows: {n_pass}/{n_win} pass ({n_pass/n_win*100:.0f}%)")
print(f"Avg return: {avg_ret:+.1f}%")
print(f"Avg Sharpe: {avg_sh:.2f}")
print(f"Worst DD:   {worst_dd:.1f}%")
print(f"Total trades: {total_trades}")

# Save CSV
df_results.write_csv("snapshots/equity_portfolio_wf.csv")
print(f"\nCSV: snapshots/equity_portfolio_wf.csv")

# ── Also run crypto-only for comparison ─────────────────────────
print("\n=== CRYPTO-ONLY (BTC/ETH/SOL) WALK-FORWARD ===")
records_crypto = []
for wi in range(total_windows):
    train_end  = TRAIN + wi * TEST
    test_start = train_end
    test_end   = min(test_start + TEST, n)
    if test_end - test_start < 30:
        continue

    close = {t: aligned[t]["close"].to_numpy() for t in CRYPTO}
    high  = {t: aligned[t]["high"].to_numpy()  for t in CRYPTO}
    low   = {t: aligned[t]["low"].to_numpy()   for t in CRYPTO}
    vol   = {t: aligned[t]["vol"].to_numpy()   for t in CRYPTO}

    equity = 1.0
    peak = 1.0
    wins = 0
    total_trades = 0
    daily_rets = []

    bar = test_start
    while bar + 2 < test_end:
        scores = []
        for t in CRYPTO:
            if bar >= len(close[t]) or bar >= len(vol[t]):
                continue
            dv = vol[t][bar] * close[t][bar] if vol[t][bar] > 0 and close[t][bar] > 0 else 0.0
            scores.append((dv, t))
        scores.sort(reverse=True)
        top_syms = [t for _, t in scores[:CAP]]

        entered = False
        for sym in top_syms:
            c_arr = close[sym]
            h_arr = high[sym]
            l_arr = low[sym]
            if bar < EP + 1 or bar >= len(c_arr):
                continue
            max_close = max(c_arr[bar - EP : bar]) if bar >= EP else c_arr[bar]
            if c_arr[bar] <= max_close:
                continue
            entry_px = c_arr[bar]
            entry = entry_px * (1.0 - TAKER_FEE)
            entry_bar_next = bar + 1
            n_bars = len(c_arr)
            highest_chand = h_arr[entry_bar_next]
            highest_turtle = h_arr[entry_bar_next]
            max_bar = min(entry_bar_next + HOLD_MAX, n_bars - 1)
            exit_bar = max_bar
            for b in range(entry_bar_next, max_bar + 1):
                highest_chand = max(highest_chand, h_arr[b])
                highest_turtle = max(highest_turtle, h_arr[b])
                atr_c = atr(h_arr, l_arr, c_arr, CHAND_P, b)
                trail_chand = highest_chand - CHAND_M * atr_c
                atr_t = atr(h_arr, l_arr, c_arr, ATR_P, b)
                trail_turtle = highest_turtle - ATR_M * atr_t
                if c_arr[b] < trail_chand or c_arr[b] < trail_turtle:
                    exit_bar = b
                    break
            if exit_bar < len(c_arr):
                exit_px = c_arr[exit_bar]
                exit = exit_px * (1.0 - TAKER_FEE)
                gross_ret = exit / entry - 1.0
                bars_held = max(exit_bar - entry_bar_next, 1)
                wins += 1 if gross_ret > 0 else 0
                total_trades += 1
                equity *= (1.0 + gross_ret)
                peak = max(peak, equity)
                avg_daily = gross_ret / bars_held
                for _ in range(bars_held):
                    daily_rets.append(avg_daily)
                bar = exit_bar + 1
                entered = True
                break
        if not entered:
            bar += 1

    ret = (equity - 1.0) * 100.0
    if len(daily_rets) >= 2:
        mn = np.mean(daily_rets)
        sd = np.std(daily_rets, ddof=0)
        sharpe = mn / sd * np.sqrt(365) if sd > 0 else 0.0
    else:
        sharpe = 0.0
    mdd = (peak - equity) / peak * 100.0
    win_rate = wins / total_trades * 100.0 if total_trades > 0 else 0.0
    passed = total_trades >= MIN_TRADES and ret > 0.0
    records_crypto.append({
        "window": wi, "return_pct": ret, "sharpe": sharpe,
        "max_dd_pct": mdd, "trades": total_trades,
        "win_rate_pct": win_rate, "pass": passed,
    })
    print(f"  W{wi:02d} | {ret:+8.1f}% sh={sharpe:6.2f} DD={mdd:5.1f}% "
          f"{total_trades:4d}t {win_rate:3.0f}% {'PASS' if passed else 'FAIL'}")

df_crypto = pl.DataFrame(records_crypto)
n_win_c = len(df_crypto)
n_pass_c = df_crypto["pass"].sum()
print(f"\nCrypto-only: {n_pass_c}/{n_win_c} pass ({n_pass_c/n_win_c*100:.0f}%)")
print(f"Avg return: {df_crypto['return_pct'].mean():+.1f}%")
print(f"Avg Sharpe: {df_crypto['sharpe'].mean():.2f}")
print(f"Worst DD:   {df_crypto['max_dd_pct'].max():.1f}%")

# Comparison
combined_avg_dd = df_results["max_dd_pct"].mean()
crypto_avg_dd   = df_crypto["max_dd_pct"].mean()
combined_avg_sh = df_results["sharpe"].mean()
crypto_avg_sh   = df_crypto["sharpe"].mean()

print(f"\n=== COMPARISON ===")
print(f"MaxDD — Combined: {combined_avg_dd:.1f}% vs Crypto: {crypto_avg_dd:.1f}%  "
      f"({combined_avg_dd - crypto_avg_dd:+.1f}pp)")
print(f"Sharpe — Combined: {combined_avg_sh:.2f} vs Crypto: {crypto_avg_sh:.2f}  "
      f"({combined_avg_sh - crypto_avg_sh:+.2f})")
print(f"Pass rate — Combined: {n_pass/n_win*100:.0f}% vs Crypto: {n_pass_c/n_win_c*100:.0f}%")

# Save comparison
df_crypto.write_csv("snapshots/crypto_only_wf.csv")
print("\nCrypto-only CSV: snapshots/crypto_only_wf.csv")
