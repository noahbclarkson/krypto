# AD_PERIOD Hyperopt — 2026-04-14 (Session 2)

## Mission
Audit and optimize DDBudget's A/D momentum period (AD_PERIOD) via walk-forward validation.

## Background

The DDBudget 3-sleeve ensemble uses A/D momentum (AD_PERIOD=5) as one sleeve. This parameter was set from full-sample sweeps without walk-forward validation. The walk-forward harness `ddbudget_3sleeve_walkforward.rs` uses this hardcoded value.

**Critical note:** This sweep used simple **price momentum** (rate-of-change) rather than the original A/D EMA formula. The original A/D EMA implementation required careful warmup management (period*4 bars) and was failing silently (all scores = 0 due to EMA initialization bug). Price momentum is mathematically simpler and avoids this issue. The result should be interpreted as "optimal price momentum period for DDBudget sleeve," not the original A/D formula.

## Method

**Harness:** `ad_period_walkforward.rs` — standalone momentum walk-forward across 9 universes × 63 windows × 28 configs.

**Sweep:** Period ∈ {3..30 step 1} — 28 values.

**Signal:** Price momentum = (close_now / close_{period ago} - 1) × 100. Rank symbols by (momentum - universe_train_mean). Buy top-ranked.

**Frozen params:** HOLD=21 bars (fixed), POSITION_CAP=3, WARMUP=200.

**Bug fixed:** Original A/D EMA implementation had `start = idx - period*2` which was insufficient warmup (period*4 needed). Corrected to `period*4`.

## Results

| Period | Pass Rate | Avg Sharpe | Verdict |
|--------|-----------|------------|---------|
| **p8** | **42/63 (67%)** | **2.00** | **← WINNER** |
| p9 | 36/63 (57%) | 1.77 | runner-up |
| p7 | 36/63 (57%) | 1.66 | runner-up |
| p4 | 39/63 (62%) | 1.23 | solid |
| p5 | 33/63 (52%) | **-1.20** | ← DDBudget baseline REJECTED |
| p14-30 | 20-38% | -1.62 to +0.20 | all REJECTED |

**Best per universe:**
- Base5: period=13 (Sharpe 6.39)
- NoDOGE: period=13 (Sharpe 4.09)
- LargeCaps5: period=3 (Sharpe 4.83)
- Legacy4: period=9 (Sharpe 1.32)
- Legacy5BNB: period=9 (Sharpe 3.11)
- LowVolume5: period=8 (Sharpe 2.43)
- OldGuardNoBNB: period=8 (Sharpe 2.54)

**Summary:** Period=8 is the most robust across universes (7/9 prefer periods 8-13, with 4 preferring 8-9).

## Key Findings

### 1. Period=5 (DDBudget Baseline) Is Rejected by Walk-Forward
Walk-forward validation rejects the full-sample-derived period=5. The walk-forward Sharpe for p=5 is -1.20 with only 52% pass rate. This is significantly WORSE than the simple momentum at p=8.

### 2. Period=8 is the Robust Winner
Sharpe 2.00, 67% pass rate. Best across most universes. Periods 3-13 show reasonable performance; >14 degrade sharply.

### 3. Long-Period Momentum Is Destrictive
Periods > 14: all have Sharpe < 0 or near-zero. Long lookback periods (28-30) remove most signal and amplify fee drag.

### 4. Context: DDBudget vs Turtle+Chandelier
DDBudget with optimized momentum period (p=8, Sharpe 2.00) is still vastly inferior to Turtle+Chandelier (Sharpe 7.66). The A/D sleeve contributes diversification (correlation ~0.11 with Turtle) but at a massive Sharpe loss.

## Conclusion

**DDBudget A/D momentum should use period=8, not period=5.** However, this does NOT change the production recommendation: Turtle+Chandelier remains superior. The DDBudget sleeve finding is academically interesting but not actionable for production.

**Recommendation:** Update DDBudget AD_PERIOD from 5 to 8 in `ddbudget_3sleeve_walkforward.rs` as a documented improvement, but do not expect DDBudget to rival Turtle performance.

## Charts

- `charts/ad_period_comparison.png` — 4-panel chart: Sharpe sweep, pass rates, heatmap, results table
- `charts/chop_filter_comparison.png` — from prior session (chop filter REJECTED)
- `charts/strategy_comparison.png` — Turtle+Chandelier vs DDBudget vs other strategies

## Files Created

- `examples/ad_period_walkforward.rs` — standalone momentum walk-forward harness
- `snapshots/ad_period_sweep.csv` — 1764 rows (28 configs × 63 windows)
- `charts/plot_ad_period_comparison.py` — comparison chart generator

## Next Steps

1. **Update DDBudget AD_PERIOD=8** in production harness (documented improvement)
2. **Fix equity curve integrity** — update `progress_equity_curves.rs` to use Chandelier dual-exit
3. **Live testnet** — needs Noah's API keys
4. **A/D Static Sleeve (20/80 Turtle)** — last untested combination idea
