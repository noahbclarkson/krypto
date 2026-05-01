#!/usr/bin/env python3
"""
Regime ATR period hyperopt chart.

Reads the Rust walk-forward outputs:
  snapshots/regime_atr_hyperopt.csv          # authoritative metrics
  snapshots/regime_atr_equity/equity_AP###_LB###_T###.csv

Outputs:
  charts/comparison_chart.png

Chart rule: line graph of actual equity time-series, dynamic y-scaling,
baseline/current/winner/runner-ups on one plot.
"""
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import pandas as pd

ROOT = Path("/home/ubuntu/.openclaw/workspace-krypto/krypto")
METRICS = ROOT / "snapshots/regime_atr_hyperopt.csv"
EQUITY_DIR = ROOT / "snapshots/regime_atr_equity"
OUT = ROOT / "charts/comparison_chart.png"

CURRENT_LB = 42
CURRENT_T = 5
BASELINE = (21, 252, 0)   # old no-filter baseline
CURRENT = (12, 42, 5)     # current production regime filter / Sharpe winner in AP-only slice


def load_metrics() -> pd.DataFrame:
    df = pd.read_csv(METRICS)
    # Authoritative CSV has numeric metrics; recompute pass_pct to avoid rounded text.
    df["pass_pct"] = 100.0 * df["pass"] / df["total"].clip(lower=1)
    return df


def cfg_row(df: pd.DataFrame, cfg):
    ap, lb, t = cfg
    hit = df[(df.atr_period == ap) & (df.lookback == lb) & (df.threshold == t)]
    return hit.iloc[0] if len(hit) else None


def choose_configs(df: pd.DataFrame):
    # Single-parameter optimization slice: AP swept across 5..60 step 1,
    # with current production LB/T held fixed.
    slice_df = df[(df.lookback == CURRENT_LB) & (df.threshold == CURRENT_T)].copy()
    by_pass = slice_df.sort_values(["pass_pct", "sharpe", "ret_pct"], ascending=[False, False, False])
    by_sharpe = slice_df.sort_values(["sharpe", "pass_pct", "ret_pct"], ascending=[False, False, False])

    configs = [("Baseline no filter", *BASELINE), ("Current / Sharpe winner", *CURRENT)]

    # Add robustness/pass-rate winner, then best remaining runner-ups by Sharpe.
    for row in pd.concat([by_pass.head(1), by_sharpe.head(8)]).drop_duplicates(
        subset=["atr_period", "lookback", "threshold"]
    ).itertuples(index=False):
        cfg = (int(row.atr_period), int(row.lookback), int(row.threshold))
        if cfg in [(c[1], c[2], c[3]) for c in configs]:
            continue
        if len(configs) == 2:
            label = "Pass-rate winner"
        else:
            label = f"Runner-up {len(configs)-2}"
        configs.append((label, *cfg))
        if len(configs) >= 5:
            break
    return configs, slice_df


def load_equity(ap: int, lb: int, t: int):
    path = EQUITY_DIR / f"equity_AP{ap:03d}_LB{lb:03d}_T{t:03d}.csv"
    if not path.exists():
        print(f"missing equity: {path}")
        return None
    eq = pd.read_csv(path)
    step_col = "step" if "step" in eq.columns else "bar"
    eq = eq.rename(columns={step_col: "step"})
    eq = eq.dropna(subset=["step", "equity"])
    eq = eq[eq.equity > 0].copy()
    return eq


def drawdown(equity: pd.Series) -> pd.Series:
    peak = equity.cummax()
    return 100.0 * (equity / peak - 1.0)


def main():
    df = load_metrics()
    configs, slice_df = choose_configs(df)

    colors = ["#999999", "#00A7E1", "#00C853", "#FF9800", "#9C27B0", "#F44336"]
    linestyles = ["--", "-", "-", "-.", ":", "-"]

    curves = []
    for idx, (kind, ap, lb, t) in enumerate(configs):
        eq = load_equity(ap, lb, t)
        row = cfg_row(df, (ap, lb, t))
        if eq is None or row is None:
            continue
        label = (
            f"{kind}: AP={ap},LB={lb},T={t} "
            f"| pass {int(row['pass'])}/{int(row['total'])}, sh {row['sharpe']:.2f}, "
            f"ret {row['ret_pct']:.0f}%, final {eq.equity.iloc[-1]:.2f}x"
        )
        curves.append((label, eq, colors[idx % len(colors)], linestyles[idx % len(linestyles)]))
        print(label)

    if not curves:
        raise SystemExit("No equity curves loaded")

    plt.style.use("seaborn-v0_8-whitegrid")
    fig, (ax1, ax2) = plt.subplots(
        2, 1, figsize=(15, 10), sharex=True,
        gridspec_kw={"height_ratios": [3.2, 1.3]}
    )
    fig.suptitle(
        "Regime ATR Period Hyperopt — Turtle-only live logic (AP sweep 5..60, LB=42, T=5)",
        fontsize=14, fontweight="bold", y=0.98,
    )

    for label, eq, color, ls in curves:
        ax1.plot(eq.step, eq.equity, label=label, color=color, linestyle=ls, linewidth=1.9, alpha=0.95)
        ax2.plot(eq.step, drawdown(eq.equity), color=color, linestyle=ls, linewidth=1.2, alpha=0.85)

    ax1.set_yscale("log")
    ax1.set_ylabel("Portfolio value (log scale)")
    ax1.set_title("Equity curves — actual exported time-series", loc="left")
    ax1.grid(True, which="both", alpha=0.28)
    ax1.legend(loc="upper left", fontsize=8, framealpha=0.92)
    ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda y, _: f"{y:.1f}x" if y < 10 else f"{y:.0f}x"))

    ax2.set_ylabel("Drawdown (%)")
    ax2.set_xlabel("Walk-forward test step")
    ax2.set_title("Drawdown (linear scale)", loc="left")
    ax2.grid(True, alpha=0.28)
    ymin = min(drawdown(eq.equity).min() for _, eq, _, _ in curves)
    ax2.set_ylim(min(ymin * 1.10, -5), 2)

    pass_winner = slice_df.sort_values(["pass_pct", "sharpe"], ascending=[False, False]).iloc[0]
    sharpe_winner = slice_df.sort_values(["sharpe", "pass_pct"], ascending=[False, False]).iloc[0]
    caption = (
        f"AP range tested: 5..60 step 1. Pass-rate winner: AP={int(pass_winner.atr_period)} "
        f"({int(pass_winner['pass'])}/{int(pass_winner.total)}, Sharpe {pass_winner.sharpe:.2f}). "
        f"Sharpe/current winner: AP={int(sharpe_winner.atr_period)} "
        f"({int(sharpe_winner['pass'])}/{int(sharpe_winner.total)}, Sharpe {sharpe_winner.sharpe:.2f})."
    )
    fig.text(0.5, 0.012, caption, ha="center", fontsize=8, color="#555555")

    OUT.parent.mkdir(parents=True, exist_ok=True)
    plt.tight_layout(rect=[0, 0.035, 1, 0.965])
    plt.savefig(OUT, dpi=180, bbox_inches="tight")
    print(f"Saved {OUT}")


if __name__ == "__main__":
    main()
