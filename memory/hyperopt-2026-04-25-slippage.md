# Hyperopt Report: Slippage Sensitivity — 2026-04-25

**Parameter Audited:** `SLIPPAGE_BPS` (hardcoded assumption)
**Scope:** 21 values (0–100 bps, step=5) × 9 universes × 7 walk-forward windows = 1,323 runs
**Runtime:** 5.8s (sweep profile)

---

## Motivation

Every backtest in this project uses `SLIPPAGE_BPS = 10` as a default. This is a **magic number** with no validation behind it. The actual live slippage assumption matters because:

- Entry slippage: limit order at bar close → fills as maker ~70% of the time → 0–3 bps
- Exit slippage: Chandelier stop triggers market sell → 5–20 bps depending on volatility
- Net asymmetric: entry costs > exit benefits in trending markets

The `walk_forward.rs` config default is `slippage_bps: 10.0`. No systematic audit ever tested whether this assumption is reasonable or whether changing it would alter strategy selection.

---

## Method

**Harness:** `examples/slippage_sweep.rs` — derived from `turtle_chandelier_walkforward.rs`
- Same dual Chandelier(7, 2.30) + Turtle ATR(24, 2.0) exit mechanism
- Same dollar-volume ranking, position cap=3, min_trades=3
- **Slippage model:** Applied as `entry = close × (1 + slip_pct + fee)` and `exit = close × (1 - slip_pct - fee)` — asymmetric, directional slippage

**Parameters tested:**
```
0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 85, 90, 95, 100 bps
```

**Universes:** Base5, NoDOGE, Legacy4, Legacy5BNB, OldGuardNoBNB, LargeCaps5, Legacy3, LowVolume5, OldGuard4
**Windows:** 7 per universe (252/252 train/test walk-forward)

---

## Key Results

| Slip (bps) | Sharpe | Return% | DD%  | Pass Rate |
|-----------|--------|---------|------|-----------|
| 0 (ideal) | 4.078 | +123.0% | 30.1% | 44/63 (69.8%) |
| 10 (default) | 3.841 | +117.0% | 30.6% | 44/63 (69.8%) |
| 20 | 3.605 | +111.1% | 31.2% | 44/63 (69.8%) |
| 30 | 3.370 | +105.4% | 31.7% | 41/63 (65.1%) |
| 50 | 2.903 | +94.4% | 33.0% | 38/63 (60.3%) |
| 100 | 1.755 | +69.6% | 36.3% | 34/63 (54.0%) |

**Degradation per 10 bps:**
- Sharpe: ~−0.24 per 10 bps (≈−5.8% per 10 bps)
- Return: ~−6.0pp per 10 bps
- DD: +0.6pp per 10 bps (worsens)
- Pass rate: stable to 20 bps, degrades at 25+ bps

---

## Critical Findings

### 1. Strategy Is Robust to Slippage ≤ 20 bps
At the **current default (10 bps)**, pass rate is IDENTICAL to 0 bps (69.8%). The strategy survives with no pass rate degradation up to 20 bps. This means the current 10 bps assumption is conservative — the strategy is more robust than the backtest default suggests.

### 2. Live Slippage Estimate: ~3–15 bps (Asymmetric)
From `microstructure_analyzer.rs` (MEMORY.md 2026-04-13):
- Entry: limit order at bar close → maker fill ~70% → slip ≈ 0–3 bps
- Exit: Chandelier stop → market sell → slip ≈ 5–12 bps
- Net: asymmetric, approximately 10 bps effective on round-trip

**The current 10 bps default is well-calibrated.** It's slightly conservative (better to overcount costs than undercount).

### 3. Pass Rate Degrades at ≥25 bps
- 0–20 bps: pass rate stable at 44/63 (69.8%)
- 25+ bps: start failing LowVolume5 and NoDOGE windows
- 50 bps: only 38/63 (60.3%) pass — marginal for production viability

### 4. Max Drawdown Increases Modestly with Slippage
MaxDD goes from 30.1% (0 bps) to 36.3% (100 bps) — +6.2pp over 100 bps. The strategy's crisis-protection properties are preserved even at extreme slippage.

### 5. Equity Curve Separation Is Small
Equity at 10 bps: 1.4624 vs 0 bps: 1.4890 — only **1.8% lower final equity**. At 30 bps: 1.4107 (5.3% below 0 bps). The equity impact is much smaller than the Sharpe impact because slippage primarily reduces the Sharpe ratio through increased variance, not through catastrophic losses.

---

## Conclusion

**SLIPPAGE_BPS = 10 is a reasonable, conservative default.** No change needed.

- The strategy is robust to 20 bps without pass rate degradation
- Live slippage is estimated at ~3–15 bps asymmetric (entry < exit)
- At the current default, backtest is slightly conservative vs live expected performance
- If live slippage exceeds 25 bps, pass rate degrades significantly

**Anti-overfitting note:** This hyperopt confirms an existing assumption rather than discovering a new optimum. The 10 bps default was not wrong. The value is in knowing the degradation curve, not in changing the default.

---

## Files

- `examples/slippage_sweep.rs` — hyperopt harness (21 values × 9 universes × 7 windows)
- `snapshots/slippage_sweep.csv` — detailed per-universe/window results
- `snapshots/slippage_sweep_equity.csv` — equity curves (bar × slip_bps × mean/min/max)
- `charts/plot_slippage_comparison.py` — chart generator
- `charts/slippage_comparison.png` — output chart
- `memory/hyperopt-2026-04-25-slippage.md` — this report

---

## Anti-Overfitting Note

This hyperopt was conducted to audit an existing assumption, not to find a new optimum. The result is confirming (10 bps is reasonable) rather than changing (keep 10 bps). This is the correct use of hyperopt: validating assumptions, not fitting parameters to data.
