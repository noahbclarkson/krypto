# REGIME_ATR_PERIOD Extensive Sweep — Turtle-Only Live Path

**Date:** 2026-05-05
**Scope:** AP ∈ [1..=80 step 1] × 9 universes × 7 WF windows = 5040 runs
**Harness:** Turtle-only live exit (matches `src/live/bot.rs`)

## Fixed Params
| Param | Value | Notes |
|-------|-------|-------|
| REGIME_LOOKBACK | 42 | Production default (settled 2026-05-02) |
| ATR_RANK_THRESHOLD | 5 | Production default (settled 2026-05-04) |
| VOL_LOOKBACK | 96 | Production default |
| TURTLE_ENTRY | 21 | Production default |
| TURTLE_ATR_P | 24 | Production default |
| TURTLE_ATR_M | 2 | Production default |
| HOLD_MAX | 12 | Production default |
| POSITION_CAP | 3 | Production default |

## Top 10 Results (by pass rate)
| AP | Pass | Pass% | AvgSharpe | AvgReturn% | AvgDD% |
|----|------|-------|-----------|------------|--------|
| 63 **WINNER** | 56 | 88.9% | 5.906 | 141.33% | 26.20% |
| 64 | 56 | 88.9% | 5.561 | 142.18% | 25.36% |
| 23 | 55 | 87.3% | 5.923 | 141.79% | 28.77% |
| 78 | 54 | 85.7% | 6.595 | 135.46% | 22.38% |
| 37 | 54 | 85.7% | 6.395 | 143.08% | 26.45% |
| 17 (baseline) | 54 | 85.7% | 6.373 | 161.74% | 26.49% |
| 53 | 54 | 85.7% | 6.294 | 133.31% | 26.81% |
| 42 | 54 | 85.7% | 5.656 | 140.87% | 25.04% |
| 51 | 54 | 85.7% | 5.235 | 105.05% | 29.47% |
| 6 | 54 | 85.7% | 4.853 | 144.30% | 30.78% |

## Winner vs Baseline
| Metric | Baseline (AP=17) | Winner (AP=63) | Delta |
|--------|-----------------|--------------|-------|
| Pass | 54 / 63 | 56 / 63 | +2 |
| Pass% | 85.7% | 88.9% | +3.2pp |
| Avg Sharpe | 6.373 | 5.906 | -0.467 |
| Avg Return% | 161.74% | 141.33% | -20.41pp |
| Positive Univs | 9 / 9 | 9 / 9 | +0 |

## Recommendation
Update production: AP=17 → AP=63

## Files
- `snapshots/ap_turtle_sweep_detail.csv` — per-window detail
- `snapshots/ap_turtle_sweep_summary.csv` — aggregated by AP
- `snapshots/ap_turtle_sweep_eq_017.csv` — baseline (AP=17) equity time-series
- `snapshots/ap_turtle_sweep_eq_063.csv` — winner equity time-series
- `snapshots/ap_turtle_sweep_eq_064.csv` — runner-up 1 equity
- `snapshots/ap_turtle_sweep_eq_023.csv` — runner-up 2 equity
