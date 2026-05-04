# T58: USDT Hedge Threshold Extensive Hyperopt

**Parameter:** HEDGE_PCT — BTC 21d ATR percentile threshold for position size reduction
**Range:** 0..=100 step 1 (101 values) × 9 universes × 7 WF windows = 6363 simulations
**Size mult when active:** 0.7

## Winner
HEDGE_PCT=16: 57/63 pass (90.5%), Sharpe 5.041, Ret 95.4%, DD 23.6%, Base5 216.34x

## Baseline (75th percentile — current production)
HEDGE_PCT=75: 55/63 pass (87.3%), Sharpe 4.910, Ret 129.7%, DD 29.3%, Base5 622.98x

## Top 10
| Rank | HEDGE_PCT | Pass | Pass% | Sharpe | Return% | DD% | Base5 Eq |
|------|-----------|------|-------|--------|---------|-----|----------|
| 1 | 16 | 57/63 | 90.5% | 5.041 | 95.4% | 23.6% | 216.34x |
| 2 | 17 | 57/63 | 90.5% | 5.031 | 99.2% | 23.6% | 253.17x |
| 3 | 49 | 56/63 | 88.9% | 5.179 | 112.6% | 26.1% | 458.80x |
| 4 | 50 | 56/63 | 88.9% | 5.171 | 112.4% | 26.1% | 458.80x |
| 5 | 51 | 56/63 | 88.9% | 5.168 | 113.7% | 26.1% | 458.80x |
| 6 | 42 | 56/63 | 88.9% | 5.076 | 110.0% | 25.8% | 398.45x |
| 7 | 43 | 56/63 | 88.9% | 5.050 | 109.9% | 25.9% | 406.52x |
| 8 | 15 | 56/63 | 88.9% | 5.028 | 95.3% | 23.6% | 216.34x |
| 9 | 11 | 56/63 | 88.9% | 5.008 | 92.1% | 23.6% | 204.61x |
| 10 | 12 | 56/63 | 88.9% | 5.008 | 92.1% | 23.6% | 204.61x |
