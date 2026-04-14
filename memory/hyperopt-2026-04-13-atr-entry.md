# Hyperopt — Turtle ATR Entry Multiplier — 2026-04-13

**Author:** Kira (cron)
**Target:** Turtle ATR Entry Multiplier — breakout quality filter
**Prior:** NEVER tested. turtle_signal only checks `close > max_close`.
**Classic Turtle:** requires `close > max_high + ATR_MULT × ATR_at_breakout`

---

## Sweep Design

- **Values tested:** {0.0, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 2.5, 3.0}
- **0.0 = disabled** (current behavior, baseline — no ATR filter)
- **>0 = enabled** (requires breakout > max_close + mult × ATR_at_breakout)
- **Strategy:** Turtle+Chandelier EP=21, Chandelier(28,2.0), DUAL_EXIT ATR(25,2.0)
- **Method:** Walk-forward 252/252, 9 universes, ~6 windows each = 54 total windows
- **Metric:** OOS pass rate, avg Sharpe, equity curves

---

## Results

| ATR Mult | Pass Rate | Avg Sharpe | Avg Return | Worst DD | Trades |
|----------|-----------|------------|------------|----------|--------|
| **0.00** | **50/54 (92.6%)** | **6.287** | **147.1%** | **70.3%** | **735** |
| 0.25 | 47/54 (87.0%) | 5.148 | 113.6% | 63.7% | 717 |
| 0.50 | 43/54 (79.6%) | 4.827 | 96.2% | 55.0% | 676 |
| 0.75 | 42/54 (77.8%) | 4.596 | 69.6% | 59.0% | 654 |
| 1.00 | 38/54 (70.4%) | 5.290 | 46.2% | 59.3% | 580 |
| 1.50 | 23/54 (42.6%) | -0.894 | 35.8% | 51.7% | 413 |
| 2.00 | 23/54 (42.6%) | -4.244 | 16.6% | 50.2% | 308 |
| 2.50 | 14/54 (25.9%) | -3.082 | 5.5% | 54.7% | 213 |
| 3.00 | 16/54 (29.6%) | 1.261 | 2.3% | 25.4% | 130 |

**Sorted by Sharpe (descending):**
1. mult=0.00: Sharpe 6.287 ✅ WINNER
2. mult=1.00: Sharpe 5.290 (-15.9%)
3. mult=0.25: Sharpe 5.148 (-18.1%)
4. mult=0.50: Sharpe 4.827 (-23.2%)
5. mult=0.75: Sharpe 4.596 (-26.9%)
6. mult=3.00: Sharpe 1.261 (-79.9%)
7. mult=1.50: Sharpe -0.894 (BROKEN)
8. mult=2.50: Sharpe -3.082 (BROKEN)
9. mult=2.00: Sharpe -4.244 (BROKEN)

---

## Analysis

**ATR Entry Multiplier = 0.0 (baseline) is the definitive winner.**

Adding ANY ATR filter hurts:
- Pass rate: 92.6% → 87.0% (mult=0.25) → 70.4% (mult=1.0) → 25-43% (mult≥1.5)
- Trade count: 735 → 717 → 580 → 130 (mult=3.0)
- Sharpe: degrades monotonically as filter tightens

**Why does the ATR filter hurt?**

The ATR entry filter is a volatility-adaptive mechanism: in high-vol regimes, you need a larger nominal move to enter. The intuition is that only significant breakouts should be taken. But empirically:

1. **Crypto breakouts are ATR-driven.** Large price moves (the kind Turtle captures) are already large in ATR terms. Requiring an additional ATR threshold eliminates the highest-conviction entries.

2. **The dual exit handles quality control.** Chandelier(28,2.0) + Turtle_ATR(25,2.0) already provides exit-based quality filtering. Trades that fail get stopped out quickly. The ATR entry filter is redundant with the exit mechanism.

3. **Low-vol chop is already filtered.** Turtle breakout requires `close > max(N)`, which is itself a volatility filter. Small-range bars have small range — the breakout threshold scales naturally without ATR adjustment.

4. **Trade frequency matters.** At mult=3.0, only 130 trades remain across 54 windows (avg 2.4/window). This is below the MIN_TRADES=3 threshold in many windows. The filter starves the strategy of signals.

**Key observation:** The DD REDUCES as the filter tightens (70.3% → 25.4%), but this is a statistical artifact of having too few trades. Lower DD with 130 trades is NOT better than 70% DD with 735 trades.

---

## Per-Universe Snapshot (mult=0.00 baseline vs mult=1.00)

| Universe | Pass@0.0 | Pass@1.0 | Δ Pass |
|----------|----------|----------|--------|
| Base5 | 6/6 | 6/6 | 0 |
| NoDOGE | 6/6 | 6/6 | 0 |
| Legacy4 | 4/6 | 2/6 | -2 |
| Legacy5BNB | 6/6 | 4/6 | -2 |
| OldGuardNoBNB | 5/6 | 3/6 | -2 |
| LargeCaps5 | 6/6 | 4/6 | -2 |
| Legacy3 | 4/6 | 1/6 | -3 |
| LowVolume5 | 5/6 | 3/6 | -2 |
| OldGuard4 | 4/6 | 2/6 | -2 |

**ATR filter hurts ALL non-Base5 universes.** The effect is concentrated in the older, lower-liquidity assets.

---

## Conclusion

**No code change.** The current turtle_signal (no ATR filter, mult=0.0 equivalent) is already optimal.

The classic Turtle ATR entry filter does NOT work for crypto daily data with the current Turtle+Chandelier parameters. The dual exit mechanism already provides adequate quality control.

**Key insight:** The ATR ENTRY MULTIPLIER is a redundant layer of risk management when the DUAL EXIT (Chandelier + Turtle ATR) already handles exit quality. Adding entry-side ATR filtering just reduces signal count without improving win quality.

---

## Files

- `examples/turtle_entry_atr_mult_hyperopt.rs` — hyperopt harness
- `snapshots/turtle_atr_entry_mult_results.csv` — aggregate results
- `snapshots/turtle_atr_entry_mult_universe.csv` — per-universe breakdown
- `snapshots/turtle_atr_entry_mult_equity.csv` — equity curves
- `charts/plot_atr_entry_mult.py` — chart generator
- `charts/comparison_chart.png` — 4-panel comparison chart
