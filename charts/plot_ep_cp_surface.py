#!/usr/bin/env python3
"""
EP × CP 2D Surface Sweep — Equity Curve Comparison Chart
Reads from snapshots/ep_cp_sweep/
"""

import os
import pandas as pd
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker

OUT_DIR = "snapshots/ep_cp_sweep"
UNIVERSE = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"]

COLORS = {
    "baseline": "#888888",
    "winner":   "#00d4ff",
    "runner":   "#ffaa00",
    "runner2":  "#cc66ff",
    "runner3":  "#ff6688",
}

def normalize(s):
    base = s.iloc[0] if len(s) > 0 else 1.0
    return s / base if base > 0 else s

def load_equity_curve(ep, cp):
    """Load and average equity curves across all windows for a given EP/CP."""
    all_eq = []
    path = f"{OUT_DIR}/EP{ep:02}_CP{cp:02}.eq.csv"
    if not os.path.exists(path):
        return None
    df = pd.read_csv(path, header=None, names=["window","eq"])
    if df is None or df.empty:
        return None
    # Average per bar index across windows
    agg = df.groupby("window")["eq"].mean().reset_index()
    return agg["eq"]

def plot_comparison(top5_ep_cp, baseline_key, output_path):
    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10))

    def add_line(ep, cp, color, label, lw=1.8):
        seq = load_equity_curve(ep, cp)
        if seq is None:
            print(f"  WARNING: no equity data for EP={ep} CP={cp}")
            return
        norm = normalize(seq)
        x = list(range(len(norm)))
        ax1.plot(x, norm.values, color=color, alpha=0.85, linewidth=lw, label=label)
        peak = norm.cummax()
        dd = (norm - peak) * 100.0
        ax2.plot(x, dd.values, color=color, alpha=0.85, linewidth=lw)

    # Baseline
    add_line(baseline_key[0], baseline_key[1], COLORS["baseline"],
             f"Baseline EP={baseline_key[0]} CP={baseline_key[1]}", 1.5)

    runner_colors = [COLORS["runner"], COLORS["runner2"], COLORS["runner3"],
                     COLORS["runner"], COLORS["runner"]]
    for i, (ep, cp) in enumerate(top5_ep_cp):
        if (ep, cp) == baseline_key:
            continue
        is_winner = (i == 0)
        color = COLORS["winner"] if is_winner else runner_colors[i % len(runner_colors)]
        lw = 2.2 if is_winner else 1.5
        add_line(ep, cp, color, f"#{i+1}: EP={ep} CP={cp}", lw)

    ax1.set_title("EP x CP Surface Sweep -- Base5 Equity (BTC,ETH,SOL,XRP,ADA)", fontsize=13)
    ax1.set_ylabel("Normalized Equity (1.0 = start)", fontsize=11)
    ax1.set_xlabel("Bar index", fontsize=11)
    ax1.legend(fontsize=9, loc="upper left")
    ax1.grid(True, alpha=0.3)
    ax1.set_yscale("log")
    ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f"{v:.2f}"))

    ax2.set_title("Drawdown (%)", fontsize=11)
    ax2.set_ylabel("Drawdown (%)", fontsize=10)
    ax2.set_xlabel("Bar index", fontsize=10)
    ax2.grid(True, alpha=0.3)

    plt.tight_layout()
    fig.savefig(output_path, dpi=150, bbox_inches="tight")
    print(f"Saved: {output_path}")
    plt.close()

def plot_heatmap(summary_csv, output_path):
    df = pd.read_csv(summary_csv)
    pivot = df.pivot(index="cp", columns="ep", values="avg_sharpe")

    fig, ax = plt.subplots(figsize=(12, 8))
    im = ax.imshow(pivot.values, aspect="auto", origin="lower", cmap="viridis")
    ax.set_xticks(range(len(pivot.columns)))
    ax.set_yticks(range(len(pivot.index)))
    ax.set_xticklabels([str(int(c)) for c in pivot.columns], fontsize=9)
    ax.set_yticklabels([str(int(r)) for r in pivot.index], fontsize=9)
    ax.set_xlabel("EP (Entry Period)", fontsize=11)
    ax.set_ylabel("CP (Chandelier Period)", fontsize=11)
    ax.set_title("EP x CP Surface Sweep -- Average Sharpe Heatmap", fontsize=13)
    plt.colorbar(im, ax=ax, label="Avg Sharpe")

    # Mark baseline
    baseline_ep, baseline_cp = 21, 11
    ep_cols = [int(c) for c in pivot.columns]
    cp_rows = [int(r) for r in pivot.index]
    if baseline_ep in ep_cols and baseline_cp in cp_rows:
        col_idx = ep_cols.index(baseline_ep)
        row_idx = cp_rows.index(baseline_cp)
        ax.add_patch(plt.Rectangle((col_idx-0.5, row_idx-0.5), 1, 1,
                                   fill=False, edgecolor="white", linewidth=2.5))

    fig.tight_layout()
    fig.savefig(output_path, dpi=150, bbox_inches="tight")
    print(f"Saved heatmap: {output_path}")
    plt.close()

if __name__ == "__main__":
    summary_path = f"{OUT_DIR}/summary.csv"
    if not os.path.exists(summary_path):
        print(f"ERROR: {summary_path} not found")
        exit(1)

    df = pd.read_csv(summary_path)
    df_sorted = df.sort_values("avg_sharpe", ascending=False)

    # Top 5 (excluding baseline if in top 5)
    baseline_key = (21, 11)
    top5_rows = df_sorted.head(10)
    top5_ep_cp = list(zip(top5_rows["ep"].astype(int), top5_rows["cp"].astype(int)))

    print(f"\nTop 10 from summary.csv:")
    for i, (_, row) in enumerate(top5_rows.head(10).iterrows()):
        ep, cp = int(row["ep"]), int(row["cp"])
        sh = row["avg_sharpe"]
        ret = row["avg_ret_pct"]
        dd = row["avg_max_dd_pct"]
        passes = int(row["pass_count"])
        total = int(row["total_windows"])
        delta = (sh - df[df["ep"]==21][df["cp"]==11]["avg_sharpe"].values[0]) / df[df["ep"]==21][df["cp"]==11]["avg_sharpe"].values[0] * 100 if len(df[df["ep"]==21][df["cp"]==11]) > 0 else 0
        print(f"  #{i+1}: EP={ep:2d} CP={cp:2d}  Sharpe={sh:.4f} ({delta:+.1f}%)  Ret={ret:+.1f}%  DD={dd:.1f}%  Pass={passes}/{total}")

    baseline_sharpe = df[(df["ep"]==21) & (df["cp"]==11)]["avg_sharpe"].values
    if len(baseline_sharpe) > 0:
        print(f"\nBaseline EP=21 CP=11: Sharpe={baseline_sharpe[0]:.4f}")
    else:
        print("\nWARNING: Baseline EP=21 CP=11 not found in results!")

    comp_path = f"{OUT_DIR}/comparison_chart.png"
    heat_path = f"{OUT_DIR}/heatmap_sharpe.png"

    print(f"\nGenerating equity curve chart...")
    plot_comparison(top5_ep_cp, baseline_key, comp_path)
    print(f"Generating heatmap...")
    plot_heatmap(summary_path, heat_path)
    print("Done.")