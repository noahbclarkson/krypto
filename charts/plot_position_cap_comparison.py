#!/usr/bin/env python3
import csv
from pathlib import Path

import matplotlib.pyplot as plt

ROOT = Path(__file__).resolve().parents[1]
SUMMARY_CSV = ROOT / "snapshots" / "position_cap_sweep_summary.csv"
EQUITY_CSV = ROOT / "snapshots" / "position_cap_all_equity.csv"
OUT = ROOT / "charts" / "comparison_chart.png"
BASELINE_CAP = 3


def load_summary():
    rows = []
    with SUMMARY_CSV.open() as f:
        reader = csv.DictReader(f)
        for row in reader:
            rows.append(
                {
                    "cap": int(row["position_cap"]),
                    "avg_sharpe": float(row["avg_sharpe"]),
                    "median_sharpe": float(row["median_sharpe"]),
                    "avg_return": float(row["avg_return"]),
                    "avg_max_dd": float(row["avg_max_dd"]),
                    "avg_win_rate": float(row["avg_win_rate"]),
                    "pass_rate": float(row["pass_rate"]),
                    "total_trades": int(row["total_trades"]),
                    "positive_universes": int(row["positive_universes"]),
                    "base5_pass_rate": float(row["base5_pass_rate"]),
                    "windows": int(row["windows"]),
                }
            )
    rows.sort(
        key=lambda r: (
            -r["pass_rate"],
            -r["positive_universes"],
            -r["avg_sharpe"],
            r["avg_max_dd"],
            abs(r["cap"] - BASELINE_CAP),
        )
    )
    return rows


def load_equity():
    with EQUITY_CSV.open() as f:
        reader = csv.DictReader(f)
        steps = []
        series = {k: [] for k in reader.fieldnames if k != "step"}
        for row in reader:
            steps.append(int(row["step"]))
            for key in series:
                cell = row[key].strip()
                series[key].append(float(cell) if cell else None)
    return steps, series


def main():
    summaries = load_summary()
    steps, series = load_equity()

    selected_caps = [BASELINE_CAP]
    for row in summaries:
        if row["cap"] not in selected_caps:
            selected_caps.append(row["cap"])
        if len(selected_caps) == 4:
            break

    summary_by_cap = {row["cap"]: row for row in summaries}
    labels = {}
    winner_cap = summaries[0]["cap"]
    runner_idx = 1
    for cap in selected_caps:
        role = "Baseline" if cap == BASELINE_CAP else ("Winner" if cap == winner_cap else f"Runner-up {runner_idx}")
        if role.startswith("Runner-up"):
            runner_idx += 1
        s = summary_by_cap[cap]
        labels[f"cap_{cap}"] = (
            f"{role} CAP={cap} | pass {s['pass_rate']:.1f}% | "
            f"Sharpe {s['avg_sharpe']:.2f} | DD {s['avg_max_dd']:.1f}%"
        )

    plt.style.use("seaborn-v0_8-darkgrid")
    fig, ax = plt.subplots(figsize=(14, 8), dpi=180)

    colors = ["#1f77b4", "#d62728", "#2ca02c", "#9467bd"]
    selected_values = []
    for idx, cap in enumerate(selected_caps):
        key = f"cap_{cap}"
        values = series[key]
        x = [step for step, val in zip(steps, values) if val is not None]
        y = [val for val in values if val is not None]
        selected_values.extend(y)
        ax.plot(x, y, label=labels[key], linewidth=2.4, color=colors[idx % len(colors)])

    ymin = min(selected_values)
    ymax = max(selected_values)
    use_log = ymin > 0 and (ymax / ymin) > 20
    if use_log:
        ax.set_yscale("log")
    else:
        span = ymax - ymin
        pad = max(span * 0.06, ymax * 0.02 if ymax > 0 else 0.05)
        ax.set_ylim(ymin - pad, ymax + pad)

    ax.set_title(
        "POSITION_CAP walk-forward equity comparison\n"
        "Selection rule: pass rate > positive universes > avg Sharpe > lower drawdown",
        fontsize=15,
        pad=14,
    )
    ax.set_xlabel("Composite walk-forward step", fontsize=12)
    ax.set_ylabel("Compounded OOS equity", fontsize=12)
    ax.grid(True, alpha=0.35)
    ax.legend(loc="best", fontsize=10, frameon=True)

    baseline = summary_by_cap[BASELINE_CAP]
    winner = summary_by_cap[winner_cap]
    verdict = (
        f"Baseline CAP={BASELINE_CAP}: pass {baseline['pass_rate']:.1f}%, Sharpe {baseline['avg_sharpe']:.2f}\n"
        f"Winner CAP={winner_cap}: pass {winner['pass_rate']:.1f}%, Sharpe {winner['avg_sharpe']:.2f}, "
        f"positive universes {winner['positive_universes']}/9, y-scale {'log' if use_log else 'linear'}"
    )
    ax.text(
        0.015,
        0.985,
        verdict,
        transform=ax.transAxes,
        va="top",
        ha="left",
        fontsize=10,
        bbox={"boxstyle": "round", "facecolor": "white", "alpha": 0.85, "edgecolor": "#cccccc"},
    )

    fig.tight_layout()
    OUT.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(OUT, bbox_inches="tight")
    print(f"saved {OUT}")


if __name__ == "__main__":
    main()
