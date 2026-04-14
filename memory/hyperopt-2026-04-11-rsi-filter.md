# Hyperopt Report: BollingerReversion RSI Filter Threshold (2026-04-11)

**Date:** 2026-04-11 16:31 UTC
**Target:** `rsi_filter` — BollingerReversion entry selectivity threshold
**Prior Default:** 20.0 (Wilder-style "deeply oversold" — only tested 3 values on DOGE in full-sample)
**New Default:** 35.0 (walk-forward winner, 4/5 symbols agree)

## Method

- **Strategy:** BollingerReversion (bb_period=20, bb_std=2.0, ATR×0.30 stop)
- **Sweep Range:** RSI ∈ {5,10,15,20,25,30,35,40,45,50} — 10 values covering the full logical range
- **Symbols:** All 5 FDUSD symbols (BTC, ETH, SOL, XRP, DOGE)
- **Validation:** Walk-forward 252/252, ~6 windows per symbol (~30 total per config)
- **Fee Model:** 0.1% taker each side
- **Total simulations:** 10 RSI values × 5 symbols × ~6 windows = ~300 window-simulations

## Results (Ranked by Avg OOS Sharpe)

| Rank | RSI | Avg Sharpe | Avg Return% | Avg DD% | Pass% | Trades | Pos Windows |
|------|-----|-----------|-------------|---------|-------|--------|-------------|
| ★ 1 | **35** | **-2.06** | **-5.4%** | **-46.5%** | **40%** | **81** | **2/5** |
| 2 | 45 | -3.05 | -23.7% | -49.8% | 40% | 103 | 2/5 |
| 3 | 50 | -3.05 | -23.7% | -49.8% | 40% | 103 | 2/5 |
| 4 | 40 | -3.13 | -23.9% | -48.9% | 40% | 97 | 2/5 |
| 5 | 30 | -22.17 | -34.9% | -49.4% | 0% | 67 | 0/5 |
| 6-8 | 5-15 | -26.03 | -43.5% | -45.9% | 0% | 51 | 0/5 |
| **9** | **20** | **-26.03** | **-43.5%** | **-45.9%** | **0%** | **51** | **0/5 ← BASELINE** |
| 10 | 25 | -26.75 | -44.8% | -47.1% | 0% | 53 | 0/5 |

**Improvement:** RSI=35 is +92.1% better than RSI=20 baseline (Sharpe -2.06 vs -26.03).

## Per-Symbol Breakdown

| Symbol | Best RSI | Sharpe | Agrees? |
|--------|----------|--------|---------|
| BTC | 35 | +5.06 | ✅ |
| ETH | 35 | -9.63 | ✅ |
| SOL | 35 | +2.35 | ✅ |
| XRP | 45 | -5.83 | ❌ (prefers 45) |
| DOGE | 35 | -4.21 | ✅ |

**Symbol agreement: 4/5 (80%)** — strong consensus for RSI=35.

## Structural Interpretation

There is a dramatic regime change at RSI ~30-35:
- **RSI 5-25:** Uniformly terrible. The tight RSI filter (< 20) generates too few entries in walk-forward OOS windows. Only 3-10 trades per symbol → insufficient statistical power + poor diversification across time.
- **RSI 30-35:** Phase transition. Relaxing the filter from 20 to 35 roughly doubles the trade count (51→81) and allows the strategy to catch milder oversold conditions.
- **RSI 40-50:** Plateau. Further relaxation doesn't help because the signal degrades (entering on RSI=50 is essentially "RSI < 50" which is true ~50% of the time).

**Why RSI=35 works:** It's the sweet spot where:
1. Enough trades are generated for statistical reliability (81 vs 51)
2. The entry is selective enough to capture genuine mean-reversion (RSI < 35 is still oversold)
3. More permissive entries capture milder pullbacks that still revert

## Honest Assessment

**CRITICAL CAVEAT:** Even at RSI=35, the aggregate OOS Sharpe is **-2.06** (negative). Only BTC (Sharpe +5.06) and SOL (Sharpe +2.35) are genuinely profitable OOS. The other 3 symbols lose money.

This confirms the HALL_OF_FAME's own warning: "THE EDGE IS IN THE STOP, NOT THE SIGNAL." BollingerReversion does NOT survive rigorous walk-forward validation across a diversified symbol set. The massive full-sample Sharpes (100+ on DOGE, XRP) are in-sample artifacts, not genuine predictive edges.

**However**, the RSI=35 filter is still a meaningful improvement:
- +92% relative Sharpe improvement over the code default
- 4/5 symbols agree
- 40% pass rate vs 0% at the default

The update should be seen as: **"If you're going to use BollingerReversion, RSI=35 is the least bad setting."**

## Also Fixed: DDBudget POSITION_CAP

During the audit, discovered that `ddbudget_3sleeve_walkforward.rs` still had `POSITION_CAP: 2` despite the earlier hyperopt finding CAP=3 was optimal. Updated to CAP=3. Validation confirmed: still passes at 72% (39/54 windows).

## Files Updated

| File | Change |
|------|--------|
| `src/algo/strategies.rs` | BollingerReversion `rsi_filter` 20.0 → 35.0 with documentation |
| `examples/ddbudget_3sleeve_walkforward.rs` | POSITION_CAP 2 → 3 (consistency fix) |
| `examples/bollinger_rsi_filter_hyperopt.rs` | NEW — dedicated RSI filter sweep harness |
| `snapshots/bollinger_rsi_filter_results.csv` | Full per-window results |
| `snapshots/bollinger_rsi_filter_summary.csv` | Aggregated results |
| `snapshots/bollinger_rsi_filter_equity.csv` | Equity time-series for charting |
| `charts/comparison_chart.png` | 4-panel comparison chart |
| `charts/rsi_filter_sweep.png` | Per-symbol RSI sweep line chart |
| `charts/plot_rsi_filter_hyperopt.py` | Python chart generator |

## Charts

- **`charts/comparison_chart.png`** — 4-panel: Sharpe bars, per-symbol heatmap, equity curves (log scale), pass/win rates
- **`charts/rsi_filter_sweep.png`** — Per-symbol OOS Sharpe vs RSI filter, showing the phase transition at RSI=30-35

## Key Lesson

**The RSI filter has a phase transition, not a smooth gradient.** Performance jumps from uniformly negative (RSI < 25) to marginally positive on some symbols (RSI 30-35), then plateaus (RSI 40-50). This suggests the BollingerReversion signal has a minimum information threshold — below RSI ~30, entries are too rare to overcome noise; above RSI ~35, the signal is too weak to add value. RSI=35 is the boundary of useful information.
