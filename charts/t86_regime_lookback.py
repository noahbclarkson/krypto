#!/usr/bin/env python3
"""T86: REGIME_LOOKBACK sweep chart."""

import matplotlib.pyplot as plt
import pandas as pd
import numpy as np

# Load sweep data
df = pd.read_csv("/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t86_regime_lookback.csv")

# Create figure
fig, ax = plt.subplots(figsize=(12, 6))

# Plot equity vs LB
ax.plot(df["LB"], df["equity"], 'b-', linewidth=1, label="Equity")

# Mark baseline
baseline = df[df["LB"] == 41]["equity"].values[0]
ax.axhline(y=baseline, color='orange', linestyle='--', alpha=0.7, label=f"Baseline LB=41 ({baseline:.4f})")

# Mark winner
winner_lb = df.loc[df["equity"].idxmax(), "LB"]
winner_eq = df["equity"].max()
ax.axhline(y=winner_eq, color='green', linestyle=':', alpha=0.7, label=f"Winner LB={winner_lb} ({winner_eq:.4f})")

# Highlight region
ax.axvspan(41, 41, alpha=0.2, color='orange')
ax.axvspan(winner_lb, winner_lb, alpha=0.2, color='green')

ax.set_xlabel("REGIME_LOOKBACK", fontsize=12)
ax.set_ylabel("Equity", fontsize=12)
ax.set_title("T86: REGIME_LOOKBACK Sweep (Simple Aggregator)", fontsize=14)
ax.legend(loc="upper right")
ax.grid(True, alpha=0.3)

# Annotate key points
ax.annotate(f"Baseline\nLB=41", xy=(41, baseline), xytext=(55, baseline-0.05),
            fontsize=9, ha='center',
            arrowprops=dict(arrowstyle='->', color='orange', alpha=0.7))

ax.annotate(f"Winner\nLB={winner_lb}", xy=(winner_lb, winner_eq), xytext=(winner_lb-30, winner_eq+0.02),
            fontsize=9, ha='center',
            arrowprops=dict(arrowstyle='->', color='green', alpha=0.7))

plt.tight_layout()
plt.savefig("/home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png", dpi=150)
print("Chart saved to /home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png")
print(f"Baseline LB=41: equity={baseline:.4f}")
print(f"Winner LB={winner_lb}: equity={winner_eq:.4f}")
print(f"Delta: {(winner_eq/baseline-1)*100:.2f}%")