# Turtle ATR Multiplier Hyperopt

Date: 2026-04-12

## Hypothesis
The Turtle ATR exit (N=25) and Chandelier ATR exit (N=28) use the SAME multiplier (2.0).
But these are different mechanisms on different lookback periods.
The Turtle ATR exit might have a different optimal multiplier than Chandelier.

## Design
- Chandelier: fixed at (28, 2) (validated optimal)
- Turtle ATR period: fixed at 24 (validated optimal)
- SWEPT: Turtle ATR multiplier ∈ { ["1.0", "1.5", "2.0", "2.5", "3.0", "3.5", "4.0", "4.5", "5.0"] }
- Baseline (current): 2.0 (same as Chandelier multiplier)
- Universes: 9, Windows: ~54 total

## Results

| Rank | Mult | Avg Sharpe | Pass Rate | Avg Return | Avg DD | Trades |
|------|------|------------|-----------|------------|--------|--------|
| 1 ←BASELINE | 2 | 2.8075 | 39/54 (72%) | +49.6% | 33.4% | 762 |
| 2 | 1 | 2.5636 | 39/54 (72%) | +53.7% | 31.1% | 1013 |
| 3 | 2 | 2.5531 | 35/54 (65%) | +45.4% | 34.8% | 759 |
| 4 | 3 | 2.5531 | 35/54 (65%) | +45.4% | 34.8% | 759 |
| 5 | 3 | 2.5531 | 35/54 (65%) | +45.4% | 34.8% | 759 |
| 6 | 4 | 2.5531 | 35/54 (65%) | +45.4% | 34.8% | 759 |
| 7 | 4 | 2.5531 | 35/54 (65%) | +45.4% | 34.8% | 759 |
| 8 | 5 | 2.5531 | 35/54 (65%) | +45.4% | 34.8% | 759 |
| 9 | 1 | 1.8241 | 37/54 (69%) | +60.8% | 32.5% | 1435 |

## Winner
**TURTLE_ATR_MULT = 2.0** (avg Sharpe 2.8075, baseline 2.8075, delta=+0.00%)

## Files
- snapshots/turtle_atr_mult_sweep.csv — per-mult summary
- snapshots/turtle_atr_mult_equity.csv — equity curve data (top 3 + baseline)

Elapsed: 6.3s
