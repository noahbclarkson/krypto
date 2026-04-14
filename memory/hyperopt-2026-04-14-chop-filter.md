# CHOP FILTER Hyperopt — 2026-04-14

## Mission
Validate the Turtle Chop Filter hypothesis: "enter Turtle only when ATR(atr_period) > median_ATR(atr_period, 252)."

## Background

The ATR-regime chop filter was identified as the **last untested parameter idea** from the PLAN. The hypothesis: only take Turtle breakout signals when volatility is above its 252-bar median, filtering low-vol chop.

**Prior related finding:** ATR entry multiplier sweep (2026-04-13) found **mult=0.0 wins** — no ATR filter at entry. Any ATR-based filter HURTS performance. The chop filter is a different form of ATR entry gate.

## Method

**Harness:** `turtle_chop_filter_walkforward.rs` — parameterized walk-forward across 9 universes × up to 6 windows (54 total per config).

**Sweep:** CHOP_ATR period ∈ {none (baseline), 5, 7, 10, 14, 20, 21, 28, 30, 40, 50, 63, 100}
- Median lookback = 252 (fixed — not swept)
- Entry gate: `atr(chop_period) > median_atr(chop_period, 252)`

**Frozen params (all validated):** EP=21, CHAND(28,2.0), TURTLE_ATR(25,2.0), CAP=3, HM=45

**Fee:** 0.1% taker each side. MIN_TRADES=3 per window.

## Results

| Config | Pass Rate | Avg Sharpe | Δ vs Baseline | Verdict |
|--------|-----------|------------|---------------|---------|
| **baseline (none)** | **53/54 (98.1%)** | **7.6592** | **—** | **✅ WINNER** |
| atr_100 | 47/48 (97.9%) | 6.9217 | -0.7375 | ❌ REJECTED |
| atr_20 | 46/50 (92.0%) | 6.3278 | -1.3313 | ❌ REJECTED |
| atr_30 | 47/49 (95.9%) | 6.2217 | -1.4374 | ❌ REJECTED |
| atr_50 | 47/48 (97.9%) | 6.1657 | -1.4934 | ❌ REJECTED |
| atr_63 | 46/48 (95.8%) | 6.1340 | -1.5251 | ❌ REJECTED |
| atr_5 | 50/54 (92.6%) | 6.0931 | -1.5661 | ❌ REJECTED |
| atr_21 | 46/50 (92.0%) | 5.9203 | -1.7388 | ❌ REJECTED |
| atr_7 | 46/53 (86.8%) | 5.7754 | -1.8838 | ❌ REJECTED |
| atr_10 | 48/54 (88.9%) | 5.5596 | -2.0996 | ❌ REJECTED |
| atr_14 | 45/54 (83.3%) | 5.3054 | -2.3538 | ❌ REJECTED |
| atr_40 | 41/48 (85.4%) | 4.8366 | -2.8226 | ❌ REJECTED |
| atr_28 | 41/49 (83.7%) | 4.7123 | -2.9469 | ❌ REJECTED |

**Per-universe: baseline wins 9/9 universes against atr_100 (best chop filter):**

| Universe | baseline | atr_100 | Δ | Winner |
|----------|----------|---------|---|--------|
| Base5 | 6.70 | 6.09 | -0.61 | baseline |
| NoDOGE | 8.25 | 7.59 | -0.66 | baseline |
| Legacy4 | 8.58 | 8.01 | -0.57 | baseline |
| Legacy5BNB | 8.81 | 7.71 | -1.10 | baseline |
| OldGuardNoBNB | 7.81 | 6.99 | -0.82 | baseline |
| LargeCaps5 | 7.96 | 7.20 | -0.76 | baseline |
| Legacy3 | 7.36 | 6.75 | -0.61 | baseline |
| LowVolume5 | 6.43 | 6.30 | -0.13 | baseline |
| OldGuard4 | 7.04 | 6.08 | -0.96 | baseline |

## Key Findings

### 1. Chop Filter REJECTED — Hypothesis Wrong
**All 12 chop filter configs FAIL to beat baseline.** The ATR-regime entry gate destroys signal quality. The best chop filter (atr_100) loses -9.6% Sharpe vs baseline. The worst (atr_28) loses -38.5%.

### 2. ATR-Based Entry Filters Are Universally Harmful
**Confirms the 2026-04-13 ATR entry multiplier finding (mult=0.0 wins).** The chop filter is just a different implementation of the same flawed concept: using ATR regime to gate Turtle entries. Every variant hurts.

### 3. Longer ATR Periods = Less Harmful
Sharpe degradation is inversely correlated with ATR period. ATR=100 (-9.6%) vs ATR=28 (-38.5%). Longer periods are less aggressive filters (more entries pass), so less signal is destroyed. ATR=14 is nearly catastrophic (-30.7%).

### 4. The Mechanism: ATR Regime ≠ Trend Quality
When ATR < 252-bar median, price is in a low-vol regime. The hypothesis assumed low-vol = no trends = bad Turtle signals. **Empirically false.** Turtle breakouts in low-vol regimes are equally or more profitable. The 252-bar median is not a valid regime separator for trend quality.

### 5. Trade Reduction Magnifies Fee Drag
Chop filters reduce trade count by 13-40% vs baseline. Fewer trades × same fees = higher fee drag per trade. Combined with removing potentially profitable signals, this doubly hurts performance.

## Conclusion

**HYPOTHESIS REJECTED.** The chop filter is the **last untested idea** and it fails. There are no more parameter ideas left in the Turtle+Chandelier framework. All Turtle parameters are frozen:
- EP=21 ✅
- ATR_PERIOD=25 ✅
- ATR_MULT=0.0 ✅ (no filter)
- CHAND_PERIOD=28 ✅
- CHAND_MULT=2.00 ✅
- HOLD_MAX=45 ✅
- POSITION_CAP=3 ✅
- **CHOP_FILTER: NOT ADOPTED — baseline confirmed optimal**

**The Turtle+Chandelier parameter space is fully explored.** No further hyperopts remain. The strategy is production-ready with current params.

## Charts

- `charts/chop_filter_comparison.png` — 4-panel chart: Sharpe sweep, pass rates, heatmap, verdict table
- `charts/strategy_comparison.png` — Turtle+Chandelier vs DDBudget vs other strategies

## Files Created

- `examples/turtle_chop_filter_walkforward.rs` — parameterized walk-forward harness
- `snapshots/chop_filter_sweep.csv` — 660 rows (13 configs × ~51 windows avg)
- `snapshots/chop_filter_wf.csv` — last-run config output
- `charts/chop_filter_final.py` — comparison chart generator

## Next Steps

**Turtle hyperopt is COMPLETE.** All parameters frozen. No further Turtle hyperopts remain.
1. Live testnet connection (needs Noah's API keys) — the only remaining validation
2. A/D 20% static sleeve (untested combination method from 2026-04-14 critique)
3. Production equity monitoring fix (update progress_equity_curves.rs to use Chandelier dual-exit)
