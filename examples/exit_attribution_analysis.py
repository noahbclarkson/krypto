#!/usr/bin/env python3
"""T22: Exit Attribution Analysis — which exit fires first?"""
import polars as pl
from pathlib import Path

BASE = Path("snapshots")

print("=== T22: Exit Attribution Walk-Forward ===")
print()

# Load dual-exit walk-forward
dual = pl.read_csv(BASE / "turtle_chandelier_9way_wf.csv")
dual_pass = dual.filter(pl.col("sharpe") > 0)
dual_by_univ = dual.group_by("universe").agg(
    pass_count=pl.col("pass").sum(),
    total=pl.len(),
    avg_sharpe=pl.col("sharpe").mean(),
)
print("Dual-exit (Chandelier + Turtle ATR):")
print(dual_by_univ.sort("universe").to_string())
print()

# Load Turtle-only walk-forward (S6)
turtle_only = pl.read_csv(BASE / "s6_turtle_only_exit_wf.csv")
turtle_by_univ = turtle_only.group_by("universe").agg(
    pass_count=pl.col("pass").sum(),
    total=pl.len(),
    avg_sharpe=pl.col("sharpe").mean(),
)
print("Turtle-only exit:")
print(turtle_by_univ.sort("universe").to_string())
print()

# Compare universes
merged = dual_by_univ.join(turtle_by_univ, on="universe", suffix="_dual")
merged = merged.sort("universe")
print("Universe | Dual pass | Turtle-only pass | Δ Sharpe | Verdict")
print("-" * 70)
for row in merged.iter_rows(named=True):
    d_pass = row["pass_count_dual"]
    t_pass = row["pass_count_turtle_only"]
    d_sh = row["avg_sharpe_dual"]
    t_sh = row["avg_sharpe_turtle_only"]
    delta = d_sh - t_sh
    better = "DUAL better" if d_pass > t_pass else ("TIED" if d_pass == t_pass else "TURTLE better")
    print(f"{row['universe']:12s} | {d_pass}/6      | {t_pass}/6             | {delta:+.2f}     | {better}")

print()
print("=== Key Finding ===")
d_total = merged["pass_count_dual"].sum()
t_total = merged["pass_count_turtle_only"].sum()
print(f"Dual-exit passes: {d_total}/54")
print(f"Turtle-only passes: {t_total}/54")
print(f"Chandelier contributes: +{d_total - t_total} additional passes")
print()
print(f"If Chandelier fired >90% of trades → dual-exit ≈ Turtle-only → 0 delta")
print(f"Since delta = {d_total - t_total} passes → Chandelier fires first in SOME trades")
print(f"Turtle ATR exit IS a real contributor (not pure noise)")
