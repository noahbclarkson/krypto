#!/usr/bin/env python3
"""
Animate an equity curve with classy Rich logging and axes that grow
dynamically as new data arrive – with optional logarithmic Y scaling.

Usage
-----
# 5 trades per frame (default), 2 FPS, log‑scale
python animate_trades.py -f ./report/top/top-strategy-trades.csv --fps 2

# 10 trades per frame, linear scale
python animate_trades.py -n 10 --linear
"""
from __future__ import annotations

import argparse
import pandas as pd
import matplotlib.pyplot as plt
from matplotlib.animation import FuncAnimation
from rich.console import Console
from rich.text import Text

# ──────────────────────────── CLI ──────────────────────────────
parser = argparse.ArgumentParser(
    description="Animate trade-by-trade equity curve"
)
parser.add_argument(
    "-f", "--file",
    default="./report/top/top-strategy-trades.csv",
    help="CSV file containing trades (default: top-strategy-trades.csv)"
)
parser.add_argument(
    "--fps",
    type=float,
    default=65,
    help="Animation frames per second (default: 60 ≈ one update every 16 ms)"
)
parser.add_argument(
    "-n", "--batch",
    type=int,
    default=10,
    metavar="INT",
    help="Number of trades (points) to add each frame (default: 5)"
)

scale_grp = parser.add_mutually_exclusive_group()
scale_grp.add_argument(
    "--log",
    dest="log_scale",
    action="store_true",
    help="Use a logarithmic Y axis (default)"
)
scale_grp.add_argument(
    "--linear",
    dest="log_scale",
    action="store_false",
    help="Use a linear Y axis"
)
parser.set_defaults(log_scale=True)                       # default == log
args = parser.parse_args()

# ──────────────────────── Load & clean the trades ──────────────
df = pd.read_csv(args.file)

# Robust timestamp parsing: coerce *anything* unparseable to NaT, then drop
df["timestamp"] = pd.to_datetime(df["timestamp"], utc=True, errors="coerce")
df.dropna(subset=["timestamp"], inplace=True)

# Ensure chronological order & correct dtypes
df.sort_values("timestamp", inplace=True, ignore_index=True)
numeric_cols = ["quantity", "pnl", "pnl_pct", "equity_after_trade"]
df[numeric_cols] = df[numeric_cols].apply(pd.to_numeric, errors="coerce")

# ─────────────────────── Matplotlib setup ──────────────────────
plt.rcParams["font.family"] = "DejaVu Sans"
plt.style.use("seaborn-v0_8-darkgrid")

fig, ax = plt.subplots(figsize=(9, 4.5))
fig.autofmt_xdate()                                       # angled dates

scale_label = "log-scale" if args.log_scale else "linear scale"
ax.set_title(f"Equity over Time ({scale_label})")
ax.set_xlabel("Time")
ax.set_ylabel("Equity ($)")
ax.set_yscale("log" if args.log_scale else "linear")

line, = ax.plot([], [], linewidth=2)
xdata: list[pd.Timestamp] = []
ydata: list[float] = []

MARGIN_X = pd.Timedelta(days=1)   # left/right time padding
MARGIN_Y = 0.05                   # ±5 % padding


def _init():
    """Initialise an empty line; axis limits set on first frame."""
    line.set_data([], [])
    return (line,)


# ────────────────── Rich console for pretty logs ───────────────
console = Console()
header = Text.assemble(
    ("TIMESTAMP".ljust(22), "bold white"),
    ("SYMBOL ",            "bold white"),
    ("SIDE ",              "bold white"),
    ("   QTY".rjust(9),    "bold white"),
    ("      PNL".rjust(12), "bold white"),
    ("  PNL%".rjust(9),    "bold white"),
    ("   EQUITY".rjust(13), "bold white"),
    ("  REASON",           "bold white"),
)
console.rule("[bold]Trade Feed")
console.print(header)

# ───────────────────── frame‑by‑frame update ───────────────────


def _update(start_idx: int):
    """Add a *batch* of rows starting at start_idx."""
    end_idx = min(start_idx + args.batch, len(df))
    new_rows = df.iloc[start_idx:end_idx]

    # ── append new data & log each trade ───────────────────────
    for _, row in new_rows.iterrows():
        xdata.append(row["timestamp"])
        ydata.append(row["equity_after_trade"])

        pnl_color = "green" if row["pnl"] >= 0 else "red"
        log = Text()
        log.append(f"{row['timestamp']:%Y-%m-%d %H:%M:%S}  ", style="bold")
        log.append(f"{row['symbol']} ", style="yellow")
        log.append(f"{row['side']:<4} ")
        log.append(f"{row['quantity']:>8.2f}  ")
        log.append(f"${row['pnl']:>10.2f}  ", style=pnl_color)
        log.append(f"{row['pnl_pct']*100:>7.2f}%  ", style=pnl_color)
        log.append(f"${row['equity_after_trade']:>11.2f}  ")
        log.append(f"{row['reason']}")
        console.print(log)

    # ── refresh line & rescale axes ────────────────────────────
    line.set_data(xdata, ydata)

    xmin, xmax = min(xdata), max(xdata)
    ymin, ymax = min(ydata), max(ydata)
    ax.set_xlim(xmin - MARGIN_X, xmax + MARGIN_X)

    if args.log_scale:
        ax.set_ylim(ymin * (1 - MARGIN_Y), ymax * (1 + MARGIN_Y))
    else:
        y_range = ymax - ymin
        pad = y_range * MARGIN_Y if y_range else 1.0
        ax.set_ylim(ymin - pad, ymax + pad)

    return (line,)


# ─────────────────── launch the animation ──────────────────────
ani = FuncAnimation(
    fig,
    _update,
    frames=range(0, len(df), args.batch),   # step == batch size
    init_func=_init,
    interval=1000 / args.fps,               # ms per frame
    repeat=False,
    blit=False
)

plt.tight_layout()
plt.show()
