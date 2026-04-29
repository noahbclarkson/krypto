#!/usr/bin/env python3
"""Plot VOL_LOOKBACK production sweep equity curves.

Input: snapshots/vol_lookback_prod_equity.csv
Output: charts/comparison_chart.png
"""
from pathlib import Path
import pandas as pd
import matplotlib.pyplot as plt

ROOT = Path(__file__).resolve().parents[1]
EQ_PATH = ROOT / "snapshots" / "vol_lookback_prod_equity.csv"
SUMMARY_PATH = ROOT / "snapshots" / "vol_lookback_prod_summary.csv"
OUT_PATH = ROOT / "charts" / "comparison_chart.png"


def label_for(col: str, summary: pd.DataFrame) -> str:
    vl = int(col.split("_")[1])
    row = summary.loc[summary["vol_lookback"] == vl]
    if row.empty:
        return f"VL={vl}"
    r = row.iloc[0]
    role = "Baseline" if vl == 8 else "Runner-up"
    best = summary.sort_values(
        ["pass_count", "positive_universes", "avg_sharpe", "avg_ret_pct"],
        ascending=False,
    ).iloc[0]
    if vl == int(best["vol_lookback"]):
        role = "Baseline/Winner" if vl == 8 else "Winner"
    return (
        f"{role} VL={vl} | pass {int(r.pass_count)}/{int(r.total_count)} "
        f"| Sh {r.avg_sharpe:.2f}"
    )


def main():
    equity = pd.read_csv(EQ_PATH)
    summary = pd.read_csv(SUMMARY_PATH)
    cols = [c for c in equity.columns if c.startswith("vl_")]
    if not cols:
        raise SystemExit(f"No equity columns found in {EQ_PATH}")

    plt.style.use("seaborn-v0_8-whitegrid")
    fig, ax = plt.subplots(figsize=(14, 8), dpi=160)

    colors = ["#111827", "#d62728", "#1f77b4", "#2ca02c", "#9467bd", "#ff7f0e"]
    for i, col in enumerate(cols):
        linewidth = 2.8 if col in ("vl_8", f"vl_{int(summary.sort_values(['pass_count','positive_universes','avg_sharpe','avg_ret_pct'], ascending=False).iloc[0].vol_lookback)}") else 1.9
        ax.plot(equity["bar"], equity[col], label=label_for(col, summary), linewidth=linewidth, color=colors[i % len(colors)])

    yvals = equity[cols].to_numpy().ravel()
    yvals = yvals[pd.notna(yvals)]
    ymin, ymax = float(yvals.min()), float(yvals.max())
    pad = max((ymax - ymin) * 0.08, ymax * 0.02, 0.01)
    ax.set_ylim(max(0, ymin - pad), ymax + pad)

    ax.set_title("VOL_LOOKBACK Hyperopt — Base5 Equity Curves (Production Params)", fontsize=15, weight="bold")
    ax.set_xlabel("Daily bar index")
    ax.set_ylabel("Portfolio equity multiple")
    ax.legend(loc="best", fontsize=9, frameon=True)
    ax.grid(True, alpha=0.35)
    fig.tight_layout()
    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(OUT_PATH, bbox_inches="tight")
    print(f"saved {OUT_PATH}")


if __name__ == "__main__":
    main()
