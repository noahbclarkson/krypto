# MIN_TRADES Hyperopt — Extensive Range

Date: 2026-04-26

## Hypothesis
MIN_TRADES=3 was validated on range 1-6 only.
The range {7, 8, 10, 12, 15, 20} was never tested.
At higher thresholds, statistical reliability changes:
- Lower MIN_TRADES: more windows qualify but fewer trades (noisier)
- Higher MIN_TRADES: fewer windows qualify but more statistical weight

## Design
- Swept: [1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 15, 20]
- Current default: MIN_TRADES=3 (2026-04-16 sweep only tested 1-6)
- 9 universes × ~6 windows each
- Pass rule: total_trades >= min_trades AND return > 0

## Results

| Rank | MT | Avg Sharpe | Pass Rate | Avg Return | Avg DD | Trades |
|------|----|------------|-----------|------------|--------|--------|
| 1 ←WINNER | 1 | 2.8317 | 37/54 (69%) | +94.5% | 33.2% | 903 |
| 2 | 2 | 2.8317 | 37/54 (69%) | +94.5% | 33.2% | 903 |
| 3 ←CURRENT | 3 | 2.8317 | 37/54 (69%) | +94.5% | 33.2% | 903 |
| 4 | 4 | 2.8317 | 37/54 (69%) | +94.5% | 33.2% | 903 |
| 5 | 5 | 2.8317 | 37/54 (69%) | +94.5% | 33.2% | 903 |
| 6 | 6 | 2.8317 | 37/54 (69%) | +94.5% | 33.2% | 903 |
| 7 | 7 | 2.8317 | 37/54 (69%) | +94.5% | 33.2% | 903 |
| 8 | 8 | 2.8317 | 37/54 (69%) | +94.5% | 33.2% | 903 |
| 9 | 10 | 2.8317 | 37/54 (69%) | +94.5% | 33.2% | 903 |
| 10 | 12 | 2.8317 | 36/54 (67%) | +94.5% | 33.2% | 903 |
| 11 | 15 | 2.8317 | 31/54 (57%) | +94.5% | 33.2% | 903 |
| 12 | 20 | 2.8317 | 7/54 (13%) | +94.5% | 33.2% | 903 |

## Winner
**MIN_TRADES = 1** (avg Sharpe 2.8317, baseline MT=3 2.8317, delta=+0.00%)

## Files
- snapshots/min_trades_sweep.csv — per-MT summary
- snapshots/min_trades_equity.csv — equity curve data (top 3 + baseline)

Elapsed: 6.6s
