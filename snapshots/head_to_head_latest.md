# Head-to-Head Trend Benchmark Snapshot

- Timestamp (UTC): 2026-04-18T16:33:28.148307744+00:00
- Base universe: BTCUSDT, ETHUSDT, SOLUSDT, XRPUSDT, DOGEUSDT, ADAUSDT
- Stress universes: 9
- Candles: 3000
- Hold bars: 21
- Fee each side: 0.100%
- Resampling: 6 blocks, 4 train / 2 test

## Ranking table

| Rank | Strategy | Score | Full Return % | Trades | WF | Resamples | Universe #1 | LOO #1 | Bootstrap #1 | Win Rate % |
|------|----------|------:|--------------:|-------:|----|-----------|-------------|--------|--------------|-----------:|
| 1 | Turtle+Regime | 0.956 | 3174.2 | 397 | 4/4 | 15/15 | 9/9 | 6/6 | 59/64 | 53.1 |
| 2 | Turtle20 | 0.497 | 2735.3 | 446 | 4/4 | 14/15 | 0/9 | 0/6 | 2/64 | 50.0 |
| 3 | MaCross50 | 0.470 | 1734.1 | 300 | 4/4 | 14/15 | 0/9 | 0/6 | 0/64 | 52.7 |
| 4 | Turtle+MACD | 0.432 | 1594.3 | 172 | 2/4 | 15/15 | 0/9 | 0/6 | 0/64 | 52.9 |
| 5 | MACD+Regime | 0.430 | 994.7 | 444 | 3/4 | 13/15 | 0/9 | 0/6 | 3/64 | 45.9 |
| 6 | MomentumBreakout | 0.409 | 2785.3 | 269 | 3/4 | 12/15 | 0/9 | 0/6 | 0/64 | 50.9 |
| 7 | MACD | 0.387 | 660.7 | 688 | 2/4 | 12/15 | 0/9 | 0/6 | 0/64 | 45.1 |
| 8 | Turtle+Regime+MACD | 0.335 | 1464.9 | 157 | 1/4 | 12/15 | 0/9 | 0/6 | 0/64 | 52.2 |

## Stress-universe winners

| Universe | Symbols | Winner | Return % | Runner-up | Return % |
|----------|---------|--------|---------:|-----------|---------:|
| Base6 | BTCUSDT, ETHUSDT, SOLUSDT, XRPUSDT, DOGEUSDT, ADAUSDT | Turtle+Regime | 3174.2 | MomentumBreakout | 2785.3 |
| Legacy5 | BTCUSDT, ETHUSDT, XRPUSDT, LTCUSDT, BNBUSDT | Turtle+Regime | 1837.9 | MomentumBreakout | 1543.2 |
| LegacyCore4 | BTCUSDT, ETHUSDT, XRPUSDT, LTCUSDT | Turtle+Regime | 1386.0 | MomentumBreakout | 997.2 |
| Majors4 | BTCUSDT, ETHUSDT, BNBUSDT, LTCUSDT | Turtle+Regime | 1463.8 | Turtle20 | 1103.4 |
| AltMix6 | ETHUSDT, SOLUSDT, XRPUSDT, DOGEUSDT, ADAUSDT, LTCUSDT | Turtle+Regime | 2972.3 | MomentumBreakout | 2459.2 |
| NoSOL | BTCUSDT, ETHUSDT, XRPUSDT, DOGEUSDT, ADAUSDT, BNBUSDT | Turtle+Regime | 2987.9 | MomentumBreakout | 2923.6 |
| NoDOGE | BTCUSDT, ETHUSDT, SOLUSDT, XRPUSDT, ADAUSDT, BNBUSDT | Turtle+Regime | 2883.5 | MomentumBreakout | 2597.2 |
| LargeCaps6 | BTCUSDT, ETHUSDT, XRPUSDT, ADAUSDT, BNBUSDT, LTCUSDT | Turtle+Regime | 2437.9 | MomentumBreakout | 2156.7 |
| OldGuard6 | BTCUSDT, ETHUSDT, XRPUSDT, LTCUSDT, BNBUSDT, EOSUSDT | Turtle+Regime | 1827.9 | MomentumBreakout | 1479.1 |
