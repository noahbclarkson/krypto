#!/usr/bin/env python3
"""
Chart: ATR_RANK_THRESHOLD Extensive Sweep — Live Turtle-Only Path
Reads: snapshots/atr_rank_threshold_live_selected_equity.csv
Output: /home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png

Plots equity over time for:
  - Baseline (T=0, no ATR rank filter)
  - Winner (T=77)
  - Runner-ups (T=78, T=81, T=82, T=83)

Uses per-universe aggregated equity (mean across windows per universe) and
global average across all universes. Y-axis is dynamically scaled (no forced 0).
"""

import csv
import sys

DATA = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/atr_rank_threshold_live_selected_equity.csv"
OUT  = "/home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png"

# Thresholds to plot: [baseline, winner, runner-ups...]
PLOT_T = [0.0, 77.0, 78.0, 81.0, 82.0, 83.0]

def col_idx_for(t):
    """Return 0-based column index for equity_T_<t>."""
    # Columns: 0=universe, 1=window, 2=bar_idx, 3=equity_T_0, 4=equity_T_77, ...
    base = 3  # first equity column
    t_offset = PLOT_T.index(t)  # position in PLOT_T list
    return base + t_offset

# --- Load raw equity data ---
# Structure: {universe: {window: {t: equity_series}}}
raw = {}
with open(DATA, newline="") as f:
    reader = csv.reader(f)
    # Row 0 is header: universe,window,bar_idx,equity_T_0,equity_T_77,equity_T_78,...
    header_row0 = next(reader)
    # Row 1 is continuation header (empty first cell, then equity_T_77..83) — SKIP
    row1 = next(reader)
    assert row1[0] == '', f"Expected empty first cell in row 1, got: {row1[0]}"

    # Determine column positions
    col_univ = 0
    col_win  = 1
    col_bar  = 2
    col_map  = {t: col_idx_for(t) for t in PLOT_T}

    for row in reader:
        if len(row) < 9:
            continue
        univ = row[col_univ]
        win  = int(row[col_win])
        bar  = int(row[col_bar])

        if univ not in raw:
            raw[univ] = {}
        if win not in raw[univ]:
            raw[univ][win] = {}

        for t in PLOT_T:
            ci = col_map[t]
            if ci < len(row):
                val = float(row[ci])
            else:
                val = 1.0
            if t not in raw[univ][win]:
                raw[univ][win][t] = []
            # Extend list to reach bar index
            while len(raw[univ][win][t]) <= bar:
                raw[univ][win][t].append(1.0)
            raw[univ][win][t][bar] = val

# --- Build global average equity per threshold ---
# Aggregate across all universes and windows: mean at each bar index
print(f"Loaded {len(raw)} universes: {sorted(raw.keys())}")

# Find max bar index
max_bar = 0
for univ in raw:
    for win in raw[univ]:
        for t in PLOT_T:
            if t in raw[univ][win]:
                max_bar = max(max_bar, len(raw[univ][win][t]))

print(f"Max bar index in data: {max_bar}")

# Build global average
global_avg = {}
for t in PLOT_T:
    series = []
    for bar in range(max_bar):
        vals = []
        for univ in sorted(raw.keys()):
            for win in sorted(raw[univ].keys()):
                seq = raw[univ][win].get(t, [])
                if bar < len(seq):
                    vals.append(seq[bar])
        if vals:
            series.append(sum(vals) / len(vals))
        else:
            series.append(1.0)
    global_avg[t] = series

# --- Find global y-axis range ---
all_vals = []
for t in PLOT_T:
    all_vals.extend(global_avg[t])
y_min = min(all_vals) * 0.95
y_max = max(all_vals) * 1.05
print(f"Y-axis: {y_min:.4f} to {y_max:.4f}")

# --- Plot ---
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

fig, ax = plt.subplots(figsize=(14, 8))

COLOR_MAP = {
    0.0:  "#888888",  # grey — baseline
    77.0: "#00BF7D",  # green — winner
    78.0: "#00D4AA",  # light green — runner-up
    81.0: "#F5A623",  # amber — runner-up
    82.0: "#F7B731",  # yellow-amber — runner-up
    83.0: "#E07020",  # orange — runner-up
}
LABEL_MAP = {
    0.0:  "Baseline (T=0, no filter) — pass 0, Sharpe -3.35",
    77.0: "Winner (T=77) — 18/63 pass, Sharpe -1.26",
    78.0: "Runner-up T=78 — 18/63 pass",
    81.0: "Runner-up T=81 — 15/63 pass",
    82.0: "Runner-up T=82 — 15/63 pass",
    83.0: "Runner-up T=83 — 15/63 pass",
}

for t in PLOT_T:
    series = global_avg[t]
    ax.plot(
        range(len(series)),
        series,
        label=LABEL_MAP[t],
        color=COLOR_MAP.get(t, "#000000"),
        linewidth=2.0 if t == 77.0 else 1.0,
        alpha=0.9 if t == 77.0 else 0.6,
    )

ax.set_xlabel("Bar index (daily bars, aggregated mean across 9 universes × 7 windows)", fontsize=10)
ax.set_ylabel("Portfolio equity (mean)", fontsize=10)
ax.set_title(
    "ATR_RANK_THRESHOLD Extensive Sweep — Live Turtle-Only Path\n"
    "Range: T ∈ [0..=100 step 1] | 9 universes × 7 walk-forward windows = 6,363 runs",
    fontsize=11, pad=14
)
ax.set_ylim(y_min, y_max)
ax.set_xlim(0, max(len(global_avg[PLOT_T[0]]), 1) - 1)
ax.legend(loc="upper left", fontsize=8.5, framealpha=0.9)
ax.grid(True, linestyle="--", alpha=0.35)

# Annotate final equity for baseline and winner
for t in [0.0, 77.0]:
    series = global_avg[t]
    final_val = series[-1] if series else 1.0
    col = COLOR_MAP.get(t, "#000000")
    label_text = f"T={int(t)}: {final_val:.3f}x"
    ax.annotate(
        label_text,
        xy=(len(series) - 1, final_val),
        xytext=(5, 0),
        textcoords="offset points",
        color=col,
        fontsize=9,
        va="center",
        fontweight="bold" if t == 77.0 else "normal",
    )

plt.tight_layout()
plt.savefig(OUT, dpi=150, bbox_inches="tight")
print(f"Saved: {OUT}")