#!/usr/bin/env python3
"""Plot ATR-rank threshold hyperopt equity curves.

Reads the Rust harness time-series equity export and plots the mean walk-forward
portfolio equity for baseline, winner, and runner-up thresholds. The Y-axis is
not anchored at zero; it uses log scaling and dynamic bounds so differences are
visible even when curves are close.
"""
from pathlib import Path
import re

import matplotlib.pyplot as plt
import pandas as pd

ROOT = Path(__file__).resolve().parents[1]
EQUITY_CSV = ROOT / "snapshots" / "atr_rank_filter_prod_equity.csv"
RESULTS_CSV = ROOT / "snapshots" / "atr_rank_filter_prod_results.csv"
OUT_MAIN = ROOT / "comparison_chart.png"
OUT_COPY = ROOT / "charts" / "atr_rank_filter_comparison_chart.png"

THRESHOLDS = [0, 5, 10, 25]
LABELS = {
    0: "Baseline T=0 (no ATR-rank filter)",
    5: "Winner T=5 (robustness-first)",
    10: "Runner-up T=10",
    25: "Runner-up T=25",
}
COLORS = {0: "#4C78A8", 5: "#F58518", 10: "#54A24B", 25: "#B279A2"}


def threshold_from_col(col: str):
    m = re.search(r"_T(\d+)$", col)
    return int(m.group(1)) if m else None


def main():
    equity = pd.read_csv(EQUITY_CSV)
    results = pd.read_csv(RESULTS_CSV)

    series = {}
    for t in THRESHOLDS:
        cols = [c for c in equity.columns if threshold_from_col(c) == t]
        if not cols:
            raise RuntimeError(f"No equity columns found for threshold {t}")
        # Arithmetic mean across 9 universes × 6 windows, preserving per-step time series.
        series[t] = equity[cols].mean(axis=1, skipna=True)

    summary = (
        results.groupby("threshold")
        .agg(
            pass_count=("pass", "sum"),
            total=("pass", "count"),
            avg_ret=("return_pct", "mean"),
            avg_sharpe=("sharpe", "mean"),
            avg_dd=("max_dd_pct", "mean"),
            trades=("trades", "sum"),
        )
        .reset_index()
    )
    summary["pass_pct"] = 100 * summary.pass_count / summary.total

    fig, ax = plt.subplots(figsize=(13.5, 7.5), dpi=160)
    y_values = []
    for t in THRESHOLDS:
        s = series[t]
        row = summary.loc[summary.threshold == t].iloc[0]
        label = (
            f"{LABELS[t]} — {int(row.pass_count)}/{int(row.total)} pass, "
            f"Sharpe {row.avg_sharpe:.2f}, DD {row.avg_dd:.1f}%"
        )
        ax.plot(equity["step"], s, label=label, color=COLORS[t], linewidth=2.4)
        y_values.extend(s.dropna().tolist())

    ymin = max(min(y_values) * 0.92, 0.05)
    ymax = max(y_values) * 1.08
    ax.set_yscale("log")
    ax.set_ylim(ymin, ymax)
    ax.set_xlabel("Walk-forward test step (daily bars)")
    ax.set_ylabel("Mean portfolio equity (log scale, start=1.0)")
    ax.set_title("ATR-Rank Entry Filter Hyperopt — Corrected Fee Model\n"
                 "Baseline vs robustness winner and runner-ups across 9 universes × 6 windows")
    ax.grid(True, which="both", linestyle="--", linewidth=0.55, alpha=0.45)
    ax.legend(loc="best", fontsize=9, frameon=True)
    fig.tight_layout()
    fig.savefig(OUT_MAIN)
    fig.savefig(OUT_COPY)
    print(f"saved {OUT_MAIN}")
    print(f"saved {OUT_COPY}")


if __name__ == "__main__":
    main()
