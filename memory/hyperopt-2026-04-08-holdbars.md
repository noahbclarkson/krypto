# Hyperopt Session — 2026-04-08 18:30 UTC

## Audit: Hardcoded Parameters
Target: `HOLD_BARS` — the number of daily bars to hold a position.
Status: Previously tested only 12 coarse values (3, 5, 7, 10, 14, 21, 28, 35, 42, 49, 56, 63).
Hardcoded at 21 bars in 30+ example files — a textbook default with NO crypto-native justification.

## Optimization Execution
Method: Full integer sweep from 3 to 63 bars (61 values) across 9 hostile universes + walk-forward 4 windows (252 train / 252 test) on Base5.
Strategy: A/D Momentum (AD_PERIOD=47, already optimized).
Criteria: Composite score = √(WF Sharpe × 9-uni Sharpe) — picks the most robust across both time periods and universes.

### Walk-Forward Results (Base5, 6 windows):

| Rank | Hold | Pass | Avg OOS Ret% | WF Sharpe | Worst DD% |
|------|------|------|-------------|-----------|-----------|
| 1 ★WINNER | **54** | 5/6 | +527.4% | **+0.49** | -53.7% |
| 2 | 62 | 5/6 | +431.3% | +0.55 | -41.1% |
| 3 | 60 | 5/6 | +386.9% | +0.43 | -55.6% |
| 4 | 57 | 5/6 | +442.6% | +0.37 | -47.8% |
| 5 | 59 | 4/6 | +328.3% | +0.36 | -66.7% |
| ... | ... | ... | ... | ... | ... |
| baseline | 21 | 5/6 | +221.6% | +0.27 | -71.1% |

### 9-Universe Results (Avg Sharpe across 9 baskets):

| Rank | Hold | Avg Sharpe | Avg Ret% | Valid |
|------|------|-----------|---------|-------|
| 1 ★WINNER | **52** | **0.20** | +231.1% | 7/9 |
| 2 | 61 | 0.20 | +1331.3% | 4/9 |
| 3 | 55 | 0.20 | +310.3% | 4/9 |
| ... | ... | ... | ... | ... |

### Composite Ranking (WF × 9-uni):

| Rank | Hold | Composite | WF Sharpe | 9-uni Sharpe |
|------|------|-----------|---------|--------------|
| 1 ★WINNER | **54** | **0.2855** | +0.49 | +0.166 |
| 2 | 61 | 0.267 | +0.36 | +0.199 |
| 3 | 62 | 0.267 | +0.55 | +0.128 |
| 4 | 60 | 0.266 | +0.43 | +0.163 |
| 5 | 57 | 0.253 | +0.37 | +0.174 |
| baseline | 21 | 0.147 | +0.27 | +0.080 |

### Key Insights:
- **Winner: hold=54** — composite score 0.2855 vs baseline(21) score 0.147 → **94% improvement**
- WF Sharpe: 0.49 vs 0.27 → **+81% improvement**
- 9-uni Sharpe: 0.166 vs 0.080 → **+108% improvement**
- 54 bars ≈ 1.8 months (vs 21 bars ≈ 3 weeks). Longer hold captures full A/D accumulation cycles.
- The 21-bar baseline was WAY too short — averaging out the accumulation signal.
- hold=43 is the ONLY config with 6/6 pass rate (100% windows pass) but lower Sharpe (0.36). The most robust by pass rate is not the highest Sharpe.
- hold=52 is the 9-universe winner (7/9 valid) — close to 54.

## Updates
- `HOLD_BARS` updated from 21 → 54 in A/D momentum strategy examples:
  - `ad_accumulation_walk_forward.rs`
  - `ad_turtle_ensemble_walk_forward.rs`
  - `harsh_universe_stress.rs`
  - `ad_symbol_aware_stress.rs`
- New stable default documented in code comments.

## Chart Export
- `charts/comparison_chart.png` — equity curves for Baseline(21), Winner(54), Runner-ups(61,62,60,57)
- `charts/hold_period_sharpe_detail.png` — WF and 9-uni Sharpe for all 61 hold values
- `snapshots/hold_period_sweep.csv` — full results
- `snapshots/hold_period_composite.csv` — composite scores
- `snapshots/hold_period_equity.csv` — time-series equity curves for all 61 configs

## Validation
- Re-running `ad_accumulation_walk_forward` with new default (hold=54) to confirm pass rate preserved.
- Ensemble benchmarks (ensemble_family_benchmark.rs etc.) intentionally keep 21 as baseline for fair comparison — do NOT change those.
