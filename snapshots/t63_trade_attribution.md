# T63: Per-Trade PnL Attribution

Generated: 2026-05-05 12:20 UTC

## Scope

Production Turtle-only live path:
- EP=21, Turtle ATR(24, 2.0), HOLD_MAX=12
- POSITION_CAP=3, VOL_LOOKBACK=92
- Regime gate AP=17 / LB=42 / ATR_RANK_T=5
- 10 bps taker fee per side
- Warmup=300 bars, matching `examples/turtle_only_equity.rs`

## Headline

| Metric | Value |
|---|---:|
| Trades | 156 |
| Compounded equity | 176.79x |
| Gross additive PnL | +698.6% |
| Fee drag | 32.6% |
| Net additive PnL | +666.1% |
| Fees / gross | 4.7% |
| Win rate | 84/156 = 53.8% |
| Avg win | +13.76% |
| Avg loss | -6.81% |
| Win/loss ratio | 2.02x |
| Max consecutive losing trades | 5 |
| Max consecutive attributed losing bars | 13 |

## Equity Concentration

| Bucket | Additive net | Log-return share | Equity without bucket |
|---|---:|---:|---:|
| Top 1 trade | +100.0% | 13.4% | 88.40x |
| Top 3 trades | +188.5% | 27.5% | 42.54x |
| Top 5 trades | +252.1% | 38.2% | 24.51x |
| Top 10 trades | +384.4% | 60.9% | 7.58x |
| Top 20 trades | +601.6% | 98.8% | 1.06x |
| Top 30 trades | +776.6% | 130.0% | 0.21x |

Interpretation: the strategy is not literally top-5-only, but it is strongly convex/trend-following. Removing the top five trades still leaves 24.5x equity; removing the top ten cuts equity to 7.6x. Top 20 winners explain essentially all positive log-return, with the remaining 136 trades roughly flat after losses/fees.

## Distribution

| Bucket | Trades | Net | Avg/trade |
|---|---:|---:|---:|
| >20% | 19 | +582.3% | +30.65% |
| 10-20% | 29 | +412.0% | +14.21% |
| 5-10% | 18 | +118.4% | +6.58% |
| 2-5% | 10 | +34.0% | +3.40% |
| 0-2% | 8 | +9.4% | +1.18% |
| -2-0% | 13 | -16.0% | -1.23% |
| -5--2% | 16 | -52.7% | -3.29% |
| -10--5% | 26 | -188.1% | -7.23% |
| -20--10% | 15 | -187.9% | -12.52% |
| <-20% | 2 | -45.5% | -22.74% |

## Exit Attribution

| Exit | Trades | Share | Net | Avg/trade |
|---|---:|---:|---:|---:|
| Turtle ATR | 138 | 88.5% | +487.2% | +3.53% |
| Max-hold | 18 | 11.5% | +178.9% | +9.94% |
| End-data | 0 | 0.0% | 0.0% | 0.0% |

## Top Trades

1. DOGEUSDT 1b +100.00% net / +100.40% gross [Turtle]
2. DOGEUSDT 1b +49.87% / +50.17% [Turtle]
3. ADAUSDT 1b +38.65% / +38.92% [Turtle]
4. ETHUSDT 6b +35.20% / +35.48% [Turtle]
5. DOGEUSDT 1b +28.38% / +28.63% [Turtle]

## Worst Trades

1. DOGEUSDT 1b -22.92% net / -22.76% gross [Turtle]
2. DOGEUSDT 1b -22.55% / -22.40% [Turtle]
3. ETHUSDT 2b -17.72% / -17.55% [Turtle]
4. ADAUSDT 3b -14.07% / -13.90% [Turtle]
5. SOLUSDT 2b -13.91% / -13.74% [Turtle]

## Verdict

T63 trust finding: current leader has a real distributed edge across 156 trades, not a single-trade mirage, but returns are highly convex. Top five trades produce 38% of log-return; top ten produce 61%. This is acceptable for trend-following but confirms fragility to missing rare breakouts. Execution readiness should preserve breakout capture and avoid filters that accidentally skip the largest trend days.

CSV: `snapshots/t63_trade_attribution.csv`
