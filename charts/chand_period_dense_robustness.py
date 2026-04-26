#!/usr/bin/env python3
"""
CHAND_PERIOD Dense Sweep — Walk-Forward Robustness Validation
==============================================================
For the top 3 CP candidates (CP=42, CP=43, CP=41) vs baseline CP=7,
break down pass rate by universe and by window to assess robustness.
"""
import pandas as pd
import os

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
sweep = pd.read_csv(os.path.join(SCRIPT_DIR, "..", "snapshots", "chand_period_dense_sweep.csv"))

candidates = [7, 41, 42, 43]
universes = ["Base5","NoDOGE","LargeCaps5","Legacy4","Legacy5BNB","OldGuardNoBNB","Legacy3","LowVolume5","OldGuard4"]
windows = list(range(6))

print("=" * 70)
print("PER-UNIVERSE PASS RATE COMPARISON")
print("=" * 70)
print(f"{'':12} | {'CP=7':>6} | {'CP=41':>6} | {'CP=42':>6} | {'CP=43':>6}")
print("-" * 55)
for u in universes:
    row = {}
    for cp in candidates:
        sub = sweep[(sweep["cp"] == cp) & (sweep["universe"] == u)]
        passed = sub["pass"].sum()
        total = len(sub)
        rate = passed / total * 100 if total > 0 else 0
        row[cp] = rate
    print(f"{u:12} | {row[7]:>5.1f}% | {row[41]:>5.1f}% | {row[42]:>5.1f}% | {row[43]:>5.1f}%")
print("-" * 55)
# Totals
for cp in candidates:
    sub = sweep[sweep["cp"] == cp]
    passed = sub["pass"].sum()
    total = len(sub)
    rate = passed / total * 100
    print(f"Global {cp:>4}: {passed}/{total} = {rate:.1f}%")

print("\n" + "=" * 70)
print("PER-WINDOW PASS RATE COMPARISON (across all universes)")
print("=" * 70)
print(f"{'Window':>8} | {'CP=7':>6} | {'CP=41':>6} | {'CP=42':>6} | {'CP=43':>6}")
print("-" * 45)
for w in windows:
    for cp in candidates:
        sub = sweep[(sweep["cp"] == cp) & (sweep["window"] == w)]
        passed = sub["pass"].sum()
        total = len(sub)
        rate = passed / total * 100 if total > 0 else 0
        print(f"W{w:>3} {cp:>4}: {passed}/{total}={rate:5.1f}%")

print("\n" + "=" * 70)
print("AGGREGATE STATS BY CP")
print("=" * 70)
for cp in candidates:
    sub = sweep[sweep["cp"] == cp]
    print(f"\nCP={cp}:")
    print(f"  Pass rate:  {sub['pass'].sum()}/{len(sub)} = {sub['pass'].mean()*100:.1f}%")
    print(f"  Avg Sharpe: {sub['sharpe'].mean():.4f}")
    print(f"  Avg Ret%:   {sub['ret_pct'].mean():.2f}%")
    print(f"  Avg DD%:    {sub['max_dd_pct'].mean():.2f}%")
    print(f"  Avg Equity: {sub['equity_final'].mean():.4f}")
    print(f"  Avg Trades: {sub['trades'].mean():.1f}")
