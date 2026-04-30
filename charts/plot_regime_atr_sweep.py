#!/usr/bin/env python3
"""
Chart regime ATR hyperparameter sweep results.
Plots equity curves for: Baseline, Winner, and Runner-ups.
"""

import pandas as pd
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import os

EQUITY_DIR = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/regime_atr_equity"
OUTPUT_PATH = "/home/ubuntu/.openclaw/workspace-krypto/krypto/charts/comparison_chart.png"

configs = [
    # (label, filename, color)
    ("Baseline (AP=21,LB=252,T=0)",  "equity_AP021_LB252_T000.csv", "#888888"),
    ("Winner (AP=12,LB=42,T=5)",     "equity_AP012_LB042_T005.csv", "#00D4AA"),
    ("Runner-up A (AP=56,LB=42,T=5)","equity_AP056_LB042_T005.csv", "#FF6B35"),
    ("Runner-up B (AP=8,LB=126,T=5)","equity_AP008_LB126_T005.csv", "#3B82F6"),
]

fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 9), gridspec_kw={'height_ratios': [3, 1]})
fig.patch.set_facecolor('#0F1117')
for ax in (ax1, ax2):
    ax.set_facecolor('#0F1117')
    ax.grid(True, alpha=0.15, linestyle='--', color='white')
    ax.tick_params(colors='#AAAAAA', labelsize=9)

# --- Top: Equity curves (log scale) ---
for label, fname, color in configs:
    path = os.path.join(EQUITY_DIR, fname)
    if not os.path.exists(path):
        print(f"WARNING: {path} not found — skipping")
        continue
    df = pd.read_csv(path)
    # Filter to steps with non-zero equity (skip flat/empty parts)
    df = df[df['step'] > 0].copy()
    if df.empty:
        print(f"WARNING: empty equity for {label}")
        continue
    ax1.plot(df['step'], df['equity'], label=label, color=color, linewidth=1.8, alpha=0.9)

ax1.set_yscale('log')
ax1.set_ylabel('Portfolio Equity (log scale)', color='#CCCCCC', fontsize=10)
ax1.set_title('Regime ATR Hyperparameter Sweep — Equity Curves\nBaseline vs Winner vs Runner-ups', 
              color='white', fontsize=13, pad=12)
ax1.legend(loc='upper left', fontsize=9, framealpha=0.3, labelcolor='white',
           facecolor='#1A1A2E', edgecolor='none')

# --- Bottom: Drawdown (linear scale) ---
for label, fname, color in configs:
    path = os.path.join(EQUITY_DIR, fname)
    if not os.path.exists(path):
        continue
    df = pd.read_csv(path)
    df = df[df['step'] > 0].copy()
    if df.empty:
        continue
    # Drawdown = 1 - equity / peak
    peak = df['equity'].cummax()
    dd = 1.0 - df['equity'] / peak
    ax2.fill_between(df['step'], 0, dd, alpha=0.25, color=color, label=label)
    ax2.plot(df['step'], dd, color=color, linewidth=1.0, alpha=0.6)

ax2.set_yscale('linear')
ax2.set_ylabel('Drawdown', color='#CCCCCC', fontsize=10)
ax2.set_xlabel('Test Step (walk-forward window index)', color='#AAAAAA', fontsize=10)
ax2.set_ylim(0, 1.0)

# Spines
for ax in (ax1, ax2):
    for spine in ax.spines.values():
        spine.set_color('#333333')

plt.tight_layout(pad=2.0)
os.makedirs(os.path.dirname(OUTPUT_PATH), exist_ok=True)
plt.savefig(OUTPUT_PATH, dpi=150, bbox_inches='tight', facecolor=fig.get_facecolor())
print(f"Saved: {OUTPUT_PATH}")
plt.close()