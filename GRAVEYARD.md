# GRAVEYARD.md — Dead Strategies

*Archived from PLAN.md and MEMORY.md. Last consolidated: 2026-04-16.*

## Killed Strategies

| Strategy | Last Test | Result | Key Reason |
|----------|-----------|--------|------------|
| BollingerReversion | 2026-04-11 | 0/288 OOS | Signal actively harmful vs random (49% random wins) |
| BOCPD regime detector | 2026-04-11 | 0% | NIG model too insensitive, run-length stuck at 13 |
| FDUSD basis carry | 2026-04-10 | 19% | Structural premium, autocorrelation 0.88 |
| Funding rate MR | 2026-04-10 | 43% | Highly autocorrelated, positive bias |
| Cross-sectional momentum | 2026-04-10 | 60% | Short side noise, long side = trend-following |
| Correlation breakout | 2026-04-11 | 50% | Underperforms random entry |
| BTC→ETH lead-lag | 2026-04-11 | 56% | Fails in bear regimes |
| Vol-contingent Chandelier | 2026-04-12 | GRAVEYARD | All configs produce identical results |
| Vol-rank A/D×Turtle switching | 2026-04-12 | 60.5% | Worse than either component alone |
| MACD+Regime | 2026-04-14 | 29% | Previously 4/4 from stale cache |
| 4h Mean Reversion | 2026-04-11 | 0/4 | Fees destroy thin edge |
| Regime-conditional allocation | 2026-04-12 | 60.5% | Dragged by weak A/D sleeve |
| A/D Static Sleeve (20/80) | 2026-04-14 | 46% | Below-random win rate, -6.2% vs Turtle |
| BTC Trend Scalar | 2026-04-14 | 0/8 configs | Baseline wins, scalar never activates |
| Turtle ATR Entry Filter | 2026-04-13 | REJECTED | mult=0.0 definitively optimal, any filter hurts |
| Chop Filter (ATR gate) | 2026-04-13 | REJECTED | ATR entry mult hyperopt: trade-starving |
| XRP 4h MR | 2026-04-11 | 0/4 | Edge destroyed by fees |
| 1h Mean Reversion | 2026-04-14 | 0/6 | ALL symbols negative Sharpe on full history |
| Turtle ATR Entry Multiplier | 2026-04-13 | mult=0.0 wins | Any non-zero filter destroys pass rate |
| Regime-conditional Turtle (Track C) | 2026-04-15 | 0/6 | 1h MR kill confirmed, ALL symbols negative |

## Why These Died

- **Mean reversion strategies** fail in crypto's high-vol regime: fees (20bp RT) destroy edges that are <50bp
- **Market-neutral strategies** (basis carry, funding MR, pair trades) require infrastructure (borrowing, perp funding) we don't have
- **Regime switching** doesn't work: A/D and Turtle win sets substantially overlap
- **Signal-first strategies** (BollingerReversion) are worse than stop-loss-only random entry
- **Short-side strategies** are a desert in crypto — momentum dominates

## The One Reliable Edge

**Directional trend-following on daily data.** Turtle breakout entry + Chandelier dual exit.
Everything else has failed OOS validation.

---

*Full graveyard entries and session context: see PLAN.md (Graveyard section) and MEMORY.md*
