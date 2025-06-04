#!/usr/bin/env python3
"""
visualize_optimization_summary.py
---------------------------------
Draw Best‑vs‑Average curves for every metric in /report/optimization_summary.csv
without the ugly y‑axis labels problem.

• Handles numbers that contain thousands‑separators (e.g. "9,297")
• Forces every numeric column to real floats
• Uses sensible y‑axis formatting so labels never overlap
• One figure per metric (solid line = Best, dashed line = Average)

Dependencies:  pandas  matplotlib
"""

from pathlib import Path
import pandas as pd
import matplotlib.pyplot as plt
from matplotlib.ticker import MaxNLocator, ScalarFormatter, PercentFormatter

CSV_PATH = Path("./report/optimization_summary.csv")   # change if needed


def load_and_clean(csv_path: Path) -> pd.DataFrame:
    """Read the CSV and convert everything that can be a number into a float."""
    if not csv_path.exists():
        raise FileNotFoundError(csv_path)

    # thousands="," eats commas such as 9,297 → 9297
    df = pd.read_csv(csv_path, thousands=",")

    # Convert ALL non‑phenotype columns to numeric wherever possible
    for col in df.columns:
        if col != "BestStrategyPhenotype":
            df[col] = pd.to_numeric(df[col], errors="coerce")

    # Drop rows where Generation is missing
    df = df.dropna(subset=["Generation"])
    df["Generation"] = df["Generation"].astype(int)

    return df


def find_metric_pairs(df: pd.DataFrame):
    """Return (base_name, best_col, avg_col) tuples automatically."""
    pairs = []
    for col in df.columns:
        if col.startswith("Best") and col != "BestStrategyPhenotype":
            base = col[4:]                # strip "Best"
            avg = f"Avg{base}"
            if avg in df.columns:
                pairs.append((base, col, avg))
    return pairs


def plot_metric(df: pd.DataFrame, base: str, best_col: str, avg_col: str) -> None:
    """Create one figure with Best and Average over generations."""
    fig, ax = plt.subplots(figsize=(9, 5), dpi=110)

    # Lines
    ax.plot(df["Generation"], df[best_col], label="Best", lw=2)
    ax.plot(df["Generation"], df[avg_col], label="Average", lw=2, ls="--")

    # Labelling & aesthetics
    ax.set_title(base, fontsize=15, pad=10)
    ax.set_xlabel("Generation")
    ax.set_ylabel(base)
    ax.grid(True, ls=":", alpha=0.35)
    ax.legend(frameon=False)

    # Make y‑axis readable (max 6 tick labels)
    ax.yaxis.set_major_locator(MaxNLocator(nbins=6))
    # For returns or rates, show as %; otherwise plain numbers
    if "Return" in base or "WinRate" in base or "Accuracy" in base:
        ax.yaxis.set_major_formatter(PercentFormatter(xmax=1.0))  # 0.28 → 28 %
    else:
        ax.yaxis.set_major_formatter(ScalarFormatter(useOffset=False))

    fig.tight_layout()


def main():
    df = load_and_clean(CSV_PATH)
    for base, best_col, avg_col in find_metric_pairs(df):
        plot_metric(df, base, best_col, avg_col)
    plt.show()


if __name__ == "__main__":
    main()
