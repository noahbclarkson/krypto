# REGIME_ATR_PERIOD (AP) Hyperopt Results

**Date:** 2026-05-04
**Sweep:** AP ∈ [1..=80] step 1, 80 values
**Universes:** 9 × 7 windows = 63 OOS windows per AP
**Strategy:** Turtle-only live path (EP=21, T=5, ATR_RANK=5, Turtle ATR exit)
**Fee:** 0.10% taker (both sides)

## Winner: AP=63

| Metric | Winner (AP=63) | Baseline (AP=12) | Delta |
|--------|----------------|----------------|-------|
| Pass Rate | 56/63 (88.9%) | 55/63 (87.3%) | +1 |
| Avg Sharpe | 6.106 | 4.910 | +1.197 |
| Avg Return | 116.1% | 129.7% | -13.5pp |
| Avg DD | 22.5% | 29.3% | -6.8pp |
| Trades | 599 | 706 | -107 |

## Top 10 AP Values
| Rank | AP | Pass | Pass% | Sharpe | Ret% | DD% | Trades |
|------|----|------|-------|--------|------|-----|--------|
| 1 | 63 | 56/63 | 88.9% | 6.106 | 116.1% | 22.5% | 599 |
| 2 | 37 | 56/63 | 88.9% | 5.843 | 130.0% | 24.9% | 659 |
| 3 | 11 | 56/63 | 88.9% | 4.727 | 135.6% | 29.6% | 710 |
| 4 | 7 | 56/63 | 88.9% | 4.389 | 128.9% | 29.2% | 743 |
| 5 | 17 | 55/63 | 87.3% | 6.371 | 150.8% | 26.4% | 682 |
| 6 | 21 | 55/63 | 87.3% | 5.762 | 130.3% | 25.4% | 650 |
| 7 | 64 | 55/63 | 87.3% | 5.591 | 115.4% | 22.5% | 611 |
| 8 | 3 | 55/63 | 87.3% | 5.223 | 145.3% | 28.6% | 737 |
| 9 | 4 | 55/63 | 87.3% | 4.997 | 137.3% | 27.7% | 741 |
| 10 | 14 | 55/63 | 87.3% | 4.947 | 119.1% | 29.1% | 686 |

## Equity Export

Exported equity curves for AP values: [63, 37, 11, 7, 17, 12]
- `snapshots/ap_hyperopt/base5_agg_ap{AP}.csv` — Base5 aggregate equity per window
- `snapshots/ap_hyperopt/{UNIVERSE}_ap{AP}_equity.csv` — per-universe equity per window
- `snapshots/ap_hyperopt_sweep.csv` — full sweep results

## Next Steps
1. **Held-out validation** — test AP=63 against pre-2021 held-out data
2. **Update `live_compatible_wf.rs`** if held-out confirms AP=63
3. **Update `config.rs`** if AP=63 is promoted
