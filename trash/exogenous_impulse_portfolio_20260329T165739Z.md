# Exogenous Impulse Portfolio Audit

Fair daily harness: signal at close, next-open entry, fixed 21-bar hold, 0.1% taker each side, top-3 strength-capped book.

## Base5

| Strategy | Return % | Sharpe | MaxDD % | Trades | Win % | Avg Active | Idle % | RiskOn % | Stress % | Neutral % |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Turtle+Regime+MACD | 1758370.7 | 6.70 | 34.3 | 185 | 58.4 | 2.07 | 14.1 | 321.5 | 649.5 | 22.0 |
| Ensemble(Majority 2/3) | 1473655.8 | 6.19 | 44.1 | 224 | 56.2 | 2.50 | 11.6 | 295.8 | 657.8 | 23.2 |
| VIX impulse inverse | 6913.7 | 4.74 | 39.8 | 324 | 51.5 | 3.62 | 10.0 | 211.6 | 209.6 | 9.3 |
| MACD+Regime | 1119984.2 | 4.06 | 34.7 | 289 | 53.3 | 3.23 | 10.7 | 241.9 | 711.3 | 13.0 |
| CrossImpact residual BTC | 1201.5 | 2.67 | 48.9 | 242 | 46.7 | 2.71 | 14.1 | 156.1 | 68.1 | 38.5 |
| SPX impulse follow | 983.5 | 2.46 | 59.8 | 325 | 47.7 | 3.63 | 10.0 | 116.2 | 140.4 | -12.3 |

## NoDOGE

| Strategy | Return % | Sharpe | MaxDD % | Trades | Win % | Avg Active | Idle % | RiskOn % | Stress % | Neutral % |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Ensemble(Majority 2/3) | 484902.0 | 7.03 | 40.8 | 174 | 55.2 | 1.95 | 15.1 | 265.3 | 573.0 | 21.1 |
| Turtle+Regime+MACD | 628695.0 | 6.59 | 34.2 | 148 | 58.8 | 1.65 | 17.7 | 223.0 | 637.2 | 27.3 |
| MACD+Regime | 81018.6 | 6.44 | 34.3 | 229 | 53.7 | 2.56 | 11.1 | 220.0 | 444.3 | 13.4 |
| VIX impulse inverse | 3702.6 | 4.00 | 38.5 | 260 | 49.6 | 2.91 | 10.8 | 179.4 | 183.0 | 7.0 |
| CrossImpact residual BTC | 3181.1 | 2.80 | 41.8 | 145 | 47.6 | 1.62 | 21.0 | 256.0 | 56.7 | 46.5 |
| SPX impulse follow | 670.1 | 2.35 | 66.1 | 260 | 45.8 | 2.91 | 10.9 | 108.8 | 118.4 | -18.1 |

## Legacy4

| Strategy | Return % | Sharpe | MaxDD % | Trades | Win % | Avg Active | Idle % | RiskOn % | Stress % | Neutral % |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Ensemble(Majority 2/3) | 20929.3 | 3.23 | 45.1 | 232 | 47.4 | 2.09 | 13.5 | 195.5 | 349.2 | 4.6 |
| Turtle+Regime+MACD | 7676.8 | 3.10 | 59.8 | 191 | 50.8 | 1.72 | 19.0 | 121.9 | 320.4 | 3.6 |
| MACD+Regime | 1826.7 | 2.87 | 46.1 | 301 | 47.8 | 2.71 | 9.6 | 139.3 | 161.4 | 0.7 |
| VIX impulse inverse | 1260.1 | 2.07 | 76.3 | 329 | 51.4 | 2.96 | 9.7 | 148.6 | 136.6 | -15.8 |
| CrossImpact residual BTC | 1172.0 | 2.01 | 49.6 | 217 | 46.5 | 1.95 | 14.7 | 33.6 | 215.4 | 13.8 |
| SPX impulse follow | 750.4 | 1.79 | 76.4 | 331 | 48.6 | 2.98 | 9.5 | 156.8 | 75.7 | -10.8 |

## Legacy5BNB

| Strategy | Return % | Sharpe | MaxDD % | Trades | Win % | Avg Active | Idle % | RiskOn % | Stress % | Neutral % |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Ensemble(Majority 2/3) | 20669.7 | 3.69 | 47.5 | 292 | 47.3 | 2.63 | 12.0 | 144.9 | 391.2 | 8.5 |
| MACD+Regime | 9747.1 | 3.58 | 38.4 | 379 | 49.1 | 3.41 | 9.3 | 156.4 | 320.9 | -9.7 |
| Turtle+Regime+MACD | 6260.0 | 3.27 | 58.8 | 244 | 50.0 | 2.20 | 17.1 | 101.5 | 324.6 | -2.3 |
| CrossImpact residual BTC | 1815.0 | 2.31 | 51.9 | 336 | 47.9 | 3.02 | 11.7 | 118.8 | 196.5 | -11.5 |
| VIX impulse inverse | 833.0 | 1.72 | 78.7 | 411 | 51.3 | 3.70 | 9.7 | 136.9 | 115.7 | -20.2 |
| SPX impulse follow | 594.8 | 1.59 | 76.4 | 414 | 48.1 | 3.73 | 9.5 | 148.8 | 64.5 | -11.5 |

## OldGuardNoBNB

| Strategy | Return % | Sharpe | MaxDD % | Trades | Win % | Avg Active | Idle % | RiskOn % | Stress % | Neutral % |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| CrossImpact residual BTC | 3342.3 | 3.71 | 41.5 | 287 | 53.3 | 2.85 | 12.7 | 126.9 | 222.9 | 9.4 |
| Ensemble(Majority 2/3) | 14286.5 | 3.67 | 56.5 | 263 | 47.9 | 2.61 | 11.2 | 194.1 | 319.2 | -5.7 |
| MACD+Regime | 1919.9 | 3.50 | 39.1 | 342 | 48.2 | 3.40 | 9.7 | 137.1 | 172.5 | -4.6 |
| Turtle+Regime+MACD | 1826.0 | 2.81 | 64.0 | 216 | 49.5 | 2.15 | 15.1 | 92.2 | 220.3 | -10.2 |
| SPX impulse follow | 1684.4 | 2.73 | 74.2 | 374 | 49.5 | 3.72 | 9.1 | 179.5 | 128.6 | -13.5 |
| VIX impulse inverse | 1639.7 | 2.57 | 66.1 | 370 | 50.5 | 3.68 | 8.9 | 159.8 | 146.0 | -13.0 |

## LargeCaps5

| Strategy | Return % | Sharpe | MaxDD % | Trades | Win % | Avg Active | Idle % | RiskOn % | Stress % | Neutral % |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Turtle+Regime+MACD | 664871.1 | 7.28 | 27.8 | 190 | 56.8 | 2.12 | 15.1 | 214.5 | 653.7 | 22.9 |
| Ensemble(Majority 2/3) | 216544.9 | 6.14 | 44.0 | 222 | 54.1 | 2.48 | 13.2 | 238.2 | 521.6 | 19.3 |
| MACD+Regime | 159715.4 | 5.92 | 38.8 | 290 | 54.1 | 3.24 | 10.8 | 252.0 | 493.0 | 3.6 |
| VIX impulse inverse | 2035.6 | 3.21 | 44.3 | 325 | 50.2 | 3.63 | 10.8 | 178.0 | 130.7 | 3.4 |
| CrossImpact residual BTC | 1950.7 | 3.06 | 47.2 | 248 | 49.6 | 2.77 | 14.6 | 173.8 | 98.7 | 36.0 |
| SPX impulse follow | 822.9 | 2.40 | 66.4 | 324 | 46.0 | 3.62 | 10.9 | 125.3 | 118.1 | -15.5 |

