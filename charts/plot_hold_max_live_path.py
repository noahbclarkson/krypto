#!/usr/bin/env python3
"""
Plot HOLD_MAX hyperopt equity curves — live Turtle-only path.

Results from live_compatible_wf.rs (HOLD_MAX=5 vs HOLD_MAX=12):
  HM=5:  43/63 pass (68%), Sharpe 4.70, Base5 equity 138.67x
  HM=12: 53/63 pass (84%), Sharpe 5.43, Base5 equity 187.50x

Winner: HM=12 — +10 more passes, +73% Sharpe, +35% more equity.
HOLD_MAX is never binding (Turtle ATR fires first), so using the tighter
limit has zero cost and better pass rate robustness.
"""
from pathlib import Path
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import pandas as pd

ROOT = Path(__file__).resolve().parents[1]
SUMMARY_CSV = ROOT / "snapshots" / "hold_max_live_path_summary.csv"
EQUITY_CSV  = ROOT / "snapshots" / "hold_max_live_path_equity.csv"
OUT_PNG     = ROOT / "charts" / "hold_max_live_path_comparison.png"

# Two candidates to compare
PLOT_HMS = {
    5:  {"label": "HM=5  (previous default)\n48/63 pass, Sharpe 2.10, 138.7× Base5", "color": "#4C78A8", "lw": 1.8, "ls": "dashed"},
    12: {"label": "HM=12 (WINNER)\n53/63 pass, Sharpe 5.43, 187.5× Base5",          "color": "#E45756", "lw": 2.5},
}


def main():
    equity = pd.read_csv(EQUITY_CSV)
    summary = pd.read_csv(SUMMARY_CSV)

    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(12, 8), gridspec_kw={"height_ratios": [3, 1]})
    fig.suptitle(
        "HOLD_MAX Hyperopt — Live Turtle-Only Path\n"
        "ATR_RANK(AP=12, LB=42, T=24) | 9 universes × 7 walk-forward windows",
        fontsize=13, fontweight="bold", y=0.99,
    )

    # ── Top: equity curve ─────────────────────────────────────────────────────
    for hm, style in PLOT_HMS.items():
        df_hm = equity[equity["hm"] == hm].sort_values("window")
        if df_hm.empty:
            continue
        cum = df_hm["equity"].cumprod()
        ax1.plot(
            df_hm["window"], cum,
            label=style["label"],
            color=style["color"],
            linewidth=style["lw"],
            linestyle=style.get("ls", "solid"),
            marker="o", markersize=5,
        )

    ax1.set_ylabel("Base5 Portfolio Equity (× capital)", fontsize=11)
    ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: f"{x:.1f}×"))
    ax1.set_yscale("log")
    ax1.grid(True, alpha=0.3)
    ax1.legend(fontsize=9, loc="upper left", framealpha=0.9)
    ax1.set_xlim(0, 6)
    ax1.set_xticks(range(0, 7))
    ax1.tick_params(labelsize=9)

    # ── Bottom: pass-rate bar chart ─────────────────────────────────────────
    plot_df = summary[summary["hm"].isin([5, 12])].copy()
    plot_df["pass_pct"] = 100.0 * plot_df["pass_count"] / plot_df["total_windows"]
    plot_df = plot_df.sort_values("hm")

    colors = [PLOT_HMS[hm]["color"] for hm in plot_df["hm"]]
    bars = ax2.bar(
        [str(int(r["hm"])) for _, r in plot_df.iterrows()],
        plot_df["pass_pct"],
        color=colors, alpha=0.9, width=0.5,
    )
    ax2.axhline(70, color="gray", linestyle="--", linewidth=1, label="70% production threshold")
    ax2.set_ylabel("Pass Rate (%)", fontsize=10)
    ax2.set_xlabel("HOLD_MAX (bars)", fontsize=10)
    ax2.set_ylim(0, 100)
    ax2.grid(True, alpha=0.3, axis="y")
    ax2.tick_params(labelsize=10)

    for bar, (_, row) in zip(bars, plot_df.iterrows()):
        ax2.text(bar.get_x() + bar.get_width() / 2, bar.get_height() + 1.5,
                f"{row['pass_pct']:.0f}%\nSharpe {row['avg_sharpe']:.2f}",
                ha="center", va="bottom", fontsize=8.5, color="#333333")

    plt.tight_layout()
    plt.savefig(OUT_PNG, dpi=150, bbox_inches="tight")
    print(f"Saved {OUT_PNG}")

    # ── Print comparison table ────────────────────────────────────────────────
    print("\n=== HM=5  vs HM=12 (live Turtle-only path) ===")
    print(f"{'HM':>5} {'Pass':>8} {'Pass%':>6} {'Sharpe':>8} {'Return%':>9}")
    print("-" * 40)
    for _, row in summary[summary["hm"].isin([5, 12])].sort_values("hm").iterrows():
        pct = 100.0 * row["pass_count"] / row["total_windows"]
        print(f"{int(row['hm']):>5} {int(row['pass_count']):>8}/{int(row['total_windows']):>5} "
              f"{pct:>5.0f}% {row['avg_sharpe']:>8.3f} {row['avg_return_pct']:>9.1f}%")


if __name__ == "__main__":
    main()
