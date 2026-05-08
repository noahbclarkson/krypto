# T86: REGIME_LOOKBACK Sweep (Exact-Live Path)

## Config
- EP=21, TurtleATR(24,2.0), HM=15, CAP=3, ATR_RANK(AP=17,T=5.0)
- HEDGE(AP=38, LB=252, PCT=0.45, SIZE=0.25)

## Sweep
- LB ∈ [5..=200], step 1 (196 values)
- Universe: Base5 (BTC/ETH/SOL/XRP/DOGE/ADA)

## Results
| LB | Equity |
|----|--------|
| 140 | 2.0776 |
| 126 | 2.0657 |
| 122 | 2.0581 |
| 123 | 2.0581 |
| 124 | 2.0581 |
| 125 | 2.0581 |
| 5 | 2.0558 |
| 6 | 2.0558 |
| 7 | 2.0558 |
| 8 | 2.0558 |
| 9 | 2.0558 |
| 10 | 2.0558 |
| 11 | 2.0558 |
| 12 | 2.0558 |
| 13 | 2.0558 |
| 14 | 2.0558 |
| 15 | 2.0558 |
| 16 | 2.0558 |
| 17 | 2.0558 |
| 18 | 2.0558 |

## Comparison
- Baseline LB=41: 2.0251
- Winner LB=140: 2.0776 (+2.59%)
- LB=41 ranks #51
