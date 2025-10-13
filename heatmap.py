#!/usr/bin/env python3
"""
heatmap.py
----------
Generates a heatmap of the best Sharpe Ratio found for each combination
of n (PLS components) and d (depth), based on the optimization summary.
"""
import pandas as pd
import matplotlib.pyplot as plt
import seaborn as sns
from pathlib import Path

CSV_PATH = Path("./report/optimization_summary.csv")

def main():
    if not CSV_PATH.exists():
        print(f"Error: Optimization summary not found at '{CSV_PATH}'")
        return

    print(f"Loading data from '{CSV_PATH}'...")
    df = pd.read_csv(CSV_PATH)

    required_cols = ["Best_n", "Best_d", "BestSharpeRatio"]
    if not all(col in df.columns for col in required_cols):
        print(f"Error: CSV must contain the columns: {required_cols}")
        return

    print("Generating pivot table for heatmap...")
    heatmap_data = df.pivot_table(
        index='Best_n',
        columns='Best_d',
        values='BestSharpeRatio',
        aggfunc='max'
    )

    if heatmap_data.empty:
        print("No data to plot. Exiting.")
        return

    plt.style.use('seaborn-v0_8-whitegrid')
    fig, ax = plt.subplots(figsize=(12, 8), dpi=100)

    sns.heatmap(
        heatmap_data,
        ax=ax,
        annot=True,
        fmt=".2f",
        cmap="viridis",
        linewidths=.5
    )

    ax.set_title("Peak Sharpe Ratio by PLS Components (n) and Lookback Depth (d)", fontsize=16, pad=20)
    ax.set_xlabel("d (Lookback Depth)", fontsize=12)
    ax.set_ylabel("n (PLS Components)", fontsize=12)

    ax.set_xticklabels([int(float(val.get_text())) for val in ax.get_xticklabels()])
    ax.set_yticklabels([int(float(val.get_text())) for val in ax.get_yticklabels()], rotation=0)

    plt.tight_layout()
    print("Displaying heatmap...")
    plt.show()

if __name__ == "__main__":
    main()
