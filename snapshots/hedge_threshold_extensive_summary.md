# USDT Hedge Threshold Extensive Hyperopt — AP17/VL92 Current Production

**Parameter:** HEDGE_PCT — BTC 21d ATR percentile threshold for position size reduction
**Range:** 0..=100 step 1 (101 values) × 9 universes × 7 WF windows = 6363 simulations
**Size mult when active:** 0.7
**Fixed production params:** EP=21, ATR(24,2.0), HOLD_MAX=12, CAP=3, VL=92, AP=17, LB=42, T=5

## Winner
HEDGE_PCT=45: 58/63 pass (92.1%), Sharpe 7.079, Ret 114.6%, DD 21.2%, Base5 114.63x

## Baseline (75th percentile — current production)
HEDGE_PCT=75: 57/63 pass (90.5%), Sharpe 6.874, Ret 132.8%, DD 24.1%, Base5 149.31x

## Top 10
| Rank | HEDGE_PCT | Pass | Pass% | Sharpe | Return% | DD% | Base5 Eq |
|------|-----------|------|-------|--------|---------|-----|----------|
| 1 | 45 | 58/63 | 92.1% | 7.079 | 114.6% | 21.2% | 114.63x |
| 2 | 65 | 58/63 | 92.1% | 6.813 | 121.1% | 23.3% | 172.76x |
| 3 | 49 | 57/63 | 90.5% | 7.100 | 116.3% | 21.4% | 155.36x |
| 4 | 50 | 57/63 | 90.5% | 7.100 | 116.3% | 21.4% | 155.36x |
| 5 | 51 | 57/63 | 90.5% | 7.100 | 116.3% | 21.4% | 155.36x |
| 6 | 48 | 57/63 | 90.5% | 7.025 | 115.1% | 21.4% | 148.69x |
| 7 | 44 | 57/63 | 90.5% | 7.024 | 113.9% | 21.1% | 118.00x |
| 8 | 46 | 57/63 | 90.5% | 7.021 | 114.2% | 21.4% | 114.63x |
| 9 | 47 | 57/63 | 90.5% | 7.017 | 114.4% | 21.4% | 116.39x |
| 10 | 52 | 57/63 | 90.5% | 6.967 | 117.6% | 22.0% | 157.41x |
