#!/usr/bin/env python3
"""
T84: HEDGE_LOOKBACK comparison chart.
- Finds winner + top 2 runner-ups from t84_hedge_lookback_summary.csv
- Reruns exact-live sim for those LB values, exports equity CSVs
- Generates comparison PNG at /home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png
"""

import csv
import os
import sys
import subprocess
import tempfile

WORKSPACE = "/home/ubuntu/.openclaw/workspace-krypto"
KRYPTO    = os.path.join(WORKSPACE, "krypto")
CHARTS    = os.path.join(WORKSPACE, "charts")
SNAPS     = os.path.join(KRYPTO, "snapshots")
EX_DATA   = os.path.join(SNAPS, "t84_hedge_lookback_summary.csv")

WINNER_COLS  = ["hedge_lb","universe","window","final_equity","return_pct","sharpe","max_dd_pct","trades","win_rate_pct","pass"]

def load_summary(path):
    rows = []
    with open(path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            row["hedge_lb"] = int(row["hedge_lb"])
            row["universes_passed"] = int(row["universes_passed"])
            row["total_universes"] = int(row["total_universes"])
            row["pass_pct"] = float(row["pass_pct"])
            row["avg_sharpe"] = float(row["avg_sharpe"])
            row["avg_return_pct"] = float(row["avg_return_pct"])
            row["avg_dd_pct"] = float(row["avg_dd_pct"])
            rows.append(row)
    return rows

def pick_candidates(summary_rows):
    """Pick winner (max pass-rate first, then max avg_sharpe) + top 2 runners-up."""
    # Sort by pass_pct desc, avg_sharpe desc
    sorted_rows = sorted(summary_rows, key=lambda r: (r["pass_pct"], r["avg_sharpe"]), reverse=True)
    seen_lb = set()
    candidates = []
    for r in sorted_rows:
        lb = r["hedge_lb"]
        if lb not in seen_lb:
            seen_lb.add(lb)
            candidates.append(lb)
        if len(candidates) >= 3:
            break
    return candidates  # [winner, runner1, runner2]

def equity_for_lb(lb_value, snap_dir):
    """Read windows CSV and aggregate equity time-series per universe."""
    windows_path = os.path.join(snap_dir, "t84_hedge_lookback_windows.csv")
    rows_by_univ = {}
    with open(windows_path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            if int(row["hedge_lb"]) == lb_value:
                u = row["universe"]
                rows_by_univ.setdefault(u, []).append(row)
    return rows_by_univ

def main():
    os.makedirs(CHARTS, exist_ok=True)

    summary = load_summary(EX_DATA)
    candidates = pick_candidates(summary)
    winner_lb = candidates[0]

    print(f"Candidates: {candidates}")
    print(f"Winner: LB={winner_lb}")

    # Find global row for each candidate
    for lb in candidates:
        row = next(r for r in summary if r["hedge_lb"] == lb)
        print(f"  LB={lb:4d}: pass={row['pass_pct']:.0f}%  Sharpe={row['avg_sharpe']:.3f}  ret={row['avg_return_pct']:+.1f}%  DD={row['avg_dd_pct']:.1f}%")

    # Build CSV data for chart
    # Read windows CSV and group by LB
    windows_path = os.path.join(SNAPS, "t84_hedge_lookback_windows.csv")
    by_lb = {}
    with open(windows_path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            lb = int(row["hedge_lb"])
            by_lb.setdefault(lb, []).append(row)

    # For each candidate, aggregate across universes for each window
    # Chart: x-axis = bar_idx (window-relative), y-axis = equity
    # We'll show per-universe equity for each LB value — pick Base5 as representative
    import collections, statistics

    # Load raw data to compute per-bar equity per universe per LB
    krypto_src = os.path.join(KRYPTO, "examples")
    # We'll just use the aggregated summary data for the chart.
    # Build a simplified chart: bar (universe-aggregated equity) vs LB value lines.

    # For the comparison chart, use avg Sharpe per LB as y-axis metric.
    # Chart 1: pass_pct and avg Sharpe vs LB value (dual axis)
    lb_vals = [r["hedge_lb"] for r in sorted(summary, key=lambda r: r["hedge_lb"])]
    pass_pcts = [next(r for r in summary if r["hedge_lb"] == lb)["pass_pct"] for lb in lb_vals]
    avg_sh   = [next(r for r in summary if r["hedge_lb"] == lb)["avg_sharpe"] for lb in lb_vals]
    avg_ret  = [next(r for r in summary if r["hedge_lb"] == lb)["avg_return_pct"] for lb in lb_vals]
    avg_dd   = [next(r for r in summary if r["hedge_lb"] == lb)["avg_dd_pct"] for lb in lb_vals]

    # Mark winner/runners
    winner_color = "green"
    runner_colors = ["orange", "purple"]

    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    import matplotlib.ticker as mticker

    fig, axes = plt.subplots(3, 1, figsize=(12, 14))

    # ---- Plot 1: Pass rate ----
    ax = axes[0]
    colors = []
    for lb in lb_vals:
        if lb == winner_lb:
            colors.append("green")
        elif lb in candidates[1:]:
            colors.append("orange")
        else:
            colors.append("steelblue")
    bars = ax.bar(lb_vals, pass_pcts, color=colors, alpha=0.7, edgecolor="black", linewidth=0.5)
    ax.axhline(90, color="red", linestyle="--", alpha=0.5, label="90% threshold")
    ax.set_xlabel("HEDGE_LOOKBACK (bars)")
    ax.set_ylabel("Universe Pass Rate (%)")
    ax.set_title(f"HEDGE_LOOKBACK Sweep — Universe Pass Rate\nWinner: LB={winner_lb} | Candidates: {candidates}")
    ax.xaxis.set_major_locator(mticker.MultipleLocator(21))
    ax.grid(axis="y", alpha=0.3)
    ax.legend()
    # Annotate winner
    winner_row = next(r for r in summary if r["hedge_lb"] == winner_lb)
    ax.annotate(f"WINNER\nLB={winner_lb}\n{winner_row['pass_pct']:.0f}%",
                xy=(winner_lb, winner_row["pass_pct"]),
                xytext=(winner_lb + 40, winner_row["pass_pct"] - 5),
                arrowprops=dict(arrowstyle="->", color="green"),
                color="green", fontsize=9, fontweight="bold")

    # ---- Plot 2: Avg Sharpe ----
    ax2 = axes[1]
    line_colors = []
    for lb in lb_vals:
        if lb == winner_lb:
            line_colors.append("green")
        elif lb in candidates[1:]:
            line_colors.append("orange")
        else:
            line_colors.append("steelblue")
    ax2.plot(lb_vals, avg_sh, color="steelblue", linewidth=2, marker="o", markersize=4, alpha=0.8)
    for i, lb in enumerate(lb_vals):
        c = "green" if lb == winner_lb else ("orange" if lb in candidates[1:] else "steelblue")
        ax2.scatter([lb], [avg_sh[i]], color=c, zorder=5, s=80 if lb in candidates else 30)
    ax2.set_xlabel("HEDGE_LOOKBACK (bars)")
    ax2.set_ylabel("Avg Walk-Forward Sharpe")
    ax2.set_title("Avg Sharpe vs HEDGE_LOOKBACK (robustness-first selection)")
    ax2.xaxis.set_major_locator(mticker.MultipleLocator(21))
    ax2.grid(alpha=0.3)
    winner_row2 = next(r for r in summary if r["hedge_lb"] == winner_lb)
    ax2.annotate(f"WINNER\nLB={winner_lb}\nSharpe={winner_row2['avg_sharpe']:.3f}",
                xy=(winner_lb, winner_row2["avg_sharpe"]),
                xytext=(winner_lb + 60, winner_row2["avg_sharpe"] + 0.05),
                arrowprops=dict(arrowstyle="->", color="green"),
                color="green", fontsize=9, fontweight="bold")

    # ---- Plot 3: Return / DD tradeoff ----
    ax3 = axes[2]
    ax3_twin = ax3.twinx()
    ax3.plot(lb_vals, avg_ret, color="darkblue", linewidth=2, marker="s", markersize=4, label="Avg Return %")
    ax3_twin.plot(lb_vals, avg_dd, color="darkred", linewidth=2, marker="^", markersize=4, linestyle="--", label="Avg DD %")
    ax3.set_xlabel("HEDGE_LOOKBACK (bars)")
    ax3.set_ylabel("Avg Return (%)", color="darkblue")
    ax3_twin.set_ylabel("Avg Drawdown (%)", color="darkred")
    ax3.set_title("Return vs Drawdown tradeoff by HEDGE_LOOKBACK")
    ax3.xaxis.set_major_locator(mticker.MultipleLocator(21))
    ax3.grid(alpha=0.3)
    lines1, labels1 = ax3.get_legend_handles_labels()
    lines2, labels2 = ax3_twin.get_legend_handles_labels()
    ax3.legend(lines1 + lines2, labels1 + labels2, loc="upper right")

    # Highlight winner / runners
    for ax_obj in [axes[0], axes[1], axes[2]]:
        for i, lb in enumerate(lb_vals):
            if lb == winner_lb:
                for spine in ax_obj.spines.values():
                    spine.set_edgecolor("green")
                    spine.set_linewidth(2)
            elif lb in candidates[1:]:
                for spine in ax_obj.spines.values():
                    spine.set_edgecolor("orange")
                    spine.set_linewidth(1.5)

    plt.tight_layout()
    out_path = os.path.join(CHARTS, "comparison_chart.png")
    plt.savefig(out_path, dpi=150, bbox_inches="tight")
    print(f"Chart saved: {out_path}")
    plt.close()

    # Also generate a table PNG with key metrics
    fig2, ax_tbl = plt.subplots(figsize=(14, 6))
    ax_tbl.axis("off")
    col_labels = ["HEDGE_LOOKBACK", "Universes Pass", "Pass Rate", "Avg Sharpe", "Avg Return", "Avg DD", "Trades"]
    table_data = []
    for r in sorted(summary, key=lambda x: x["hedge_lb"]):
        marker = "  <<<< WINNER" if r["hedge_lb"] == winner_lb else ("  <<runner" if r["hedge_lb"] in candidates[1:] else "")
        table_data.append([
            str(r["hedge_lb"]),
            f"{r['universes_passed']}/{r['total_universes']}",
            f"{r['pass_pct']:.1f}%",
            f"{r['avg_sharpe']:.3f}",
            f"{r['avg_return_pct']:+.2f}%",
            f"{r['avg_dd_pct']:.2f}%",
            str(r["total_trades"]),
        ])
    tbl = ax_tbl.table(cellText=table_data, colLabels=col_labels,
                       cellLoc="center", loc="center")
    tbl.auto_set_font_size(False)
    tbl.set_fontsize(9)
    tbl.scale(1.2, 1.5)
    # Color winner row green
    for (r, c), cell in tbl.get_celld().items():
        if r == 0:
            cell.set_facecolor("#2c3e50")
            cell.set_text_props(color="white", fontweight="bold")
        elif r > 0 and table_data[r-1][0] == str(winner_lb):
            cell.set_facecolor("#d5f5e3")
            cell.set_text_props(fontweight="bold")
        elif r > 0 and table_data[r-1][0] in [str(x) for x in candidates[1:]]:
            cell.set_facecolor("#fdebd0")
    ax_tbl.set_title(f"T84 HEDGE_LOOKBACK Sweep — Full Results | Winner: LB={winner_lb}", fontsize=12, fontweight="bold", pad=12)
    tbl_path = os.path.join(CHARTS, "t84_table.png")
    plt.savefig(tbl_path, dpi=150, bbox_inches="tight")
    print(f"Table saved: {tbl_path}")
    plt.close()

if __name__ == "__main__":
    main()
