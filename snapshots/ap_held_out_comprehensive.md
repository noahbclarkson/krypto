# AP Held-Out Validation Results

**Date:** 2026-05-04
**Purpose:** Validate top AP candidates from OOS sweep against pre-sweep held-out data
**Context:** AP=63 won OOS by pass rate (56/63 vs 55/63) but AP=17 dominates on Sharpe/Return
**Anti-overfit pattern:** AP=63 was 3rd sequential optimization — same pattern as EP=24

## Held-Out Summary (Pre-2021 data, 4 periods × 10 symbols)

| AP | Pass | Pass% | Avg Equity | Avg Sharpe | Avg DD% | Trades |
|----|------|-------|------------|------------|---------|--------|
| 7 | 3/4 | 75.0% | 1.5235x | 3.067 | 28.7% | 51 |
| 12 | 2/4 | 50.0% | 1.3645x | 3.762 | 28.0% | 50 |
| **17** | **4/4** | **100%** | **1.9481x** | **7.715** | **21.3%** | **47** |
| 37 | 3/4 | 75.0% | 1.4067x | 4.425 | 27.7% | 48 |
| 63 | 4/4 | 100% | 1.3976x | 5.721 | 26.6% | 42 |

## Decision

**Winner: AP=17** (held-out)

AP=17 dominates on ALL held-out metrics:
- Tied 1st on pass rate (4/4) with AP=63
- **Sharpe 7.715** vs AP=63's 5.721 (+34.8%)
- **Equity 1.9481x** vs AP=63's 1.3976x (+39.4%)
- **Lowest DD: 21.3%** vs AP=63's 26.6%

AP=63 won the OOS sweep by pass rate (+1 window) but failed held-out on Sharpe/Return/DD.
The pass-rate metric was misleading — AP=63's Sharpe was inflated by trade starvation.

**Updated:** `config.rs` REGIME_ATR_PERIOD = 12 → 17
**Updated:** `live_compatible_wf.rs` REGIME_ATR_PERIOD = 63 → 17

Confirmed by live_compatible_wf with AP=17:
- 55/63 pass (87.3%), Sharpe 6.371, Return 150.8%
- Base5 aggregate equity: **606.78x** (vs AP=12's 336x = +80.6% improvement)
