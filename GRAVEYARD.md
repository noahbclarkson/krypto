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
| ATR_ENTRY_MULT=0.85 (Turtle entry filter) | 2026-04-26 | REJECTED | Swept 41 values with EP=21; EM=0.85 won OOS (82.5% pass, Sharpe 5.21) but FAILED held-out (10/18 vs baseline 11/18). Same in-sample inflation pattern as EP=24. ATR_ENTRY_MULT=0.00 confirmed as definitive default.

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

## Equity Portfolio Integration — REJECTED (2026-04-16)
- Hypothesis: Adding SPY/QQQ/GLD reduces MaxDD without proportional return sacrifice
- Test: Combined 6-asset (BTC/ETH/SOL/SPY/QQQ/GLD) walk-forward vs crypto-only, 4 windows 2020-2026
- Result: REJECTED. Combined underperforms crypto-only in all metrics:
  - Sharpe: 1.05 vs 4.00 (-2.94)
  - MaxDD: 22.5% vs 5.4% (+17.1pp worse)
  - Return: +4.1% vs +28.8% (-24.7pp)
- Mechanism: BTC dominates DV ranking → CAP=3 excludes SPY/QQQ/GLD in most windows
- Verdict: Equities hurt Turtle portfolio. Keep crypto-only production.

## Unranked Portfolio Construction (2026-04-16)
- **Result:** 5/6 pass, Sharpe=0.97, DD=40.3%, WR=42.7% (vs ranked 6/6, Sharpe=2.54, DD=59.3%, WR=52.7%)
- **Key Reason:** The 2026-04-15 "374,884x equity" claim was a simulation artifact. `entry_ranking_audit.rs` did not properly track concurrent position overlap. Ranked concentrates capital in top DV symbols — the opposite of what was claimed. Concentration beats diversification in Turtle trend-following.
- **VERDICT:** Unranked portfolio construction is graveyard. Ranked (production) is validated.

## DynamicTrend EMA Signal + Chandelier Exit — REJECTED (2026-04-16)

**Hypothesis:** If Chandelier dual-exit is the quality driver, EMA(60/100) crossover signal might be equivalent to Turtle breakout when paired with the same exit.

**Method:** Side-by-side 4-universe, 24-window walk-forward. Both use Chandelier(28, 2.15) + ATR(24, 2.0) dual exit.

**Result:** Turtle wins 21/24 (87.5%) windows. DynamicTrend wins only in specific regimes:
- Base5 W01: DT +362% vs Turtle +217% (momentum peak)
- Legacy4 W00: DT +385% vs Turtle +243% (crisis window)
- Legacy4 W03: DT +155% vs Turtle +106% (specific chop regime)

**Verdict:** Signal matters, not just exit. Turtle breakout captures break-of-structure dynamics that EMA smoothing misses. Earlier entry in trending markets = better risk-adjusted returns.

**Evidence file:** `examples/dynamic_trend_chandelier_walkforward.rs`, `snapshots/dynamic_trend_chandelier_wf.csv`

## T7: BTC/ETH Correlation Entry Filter (2026-04-25)

**Hypothesis:** 2026 YTD failure may be BTC-led divergence. ALT breakouts fire but get stopped by Chandelier when BTC doesn't confirm. BTC/ETH trend confirmation filter might reduce whipsaw.

**Test:** 4 filter variants × Base5 (6 windows) + all 9 universes × 6 windows = 240 runs total.

**Result:** All 3 filter variants lose to baseline on every metric.

| Filter | Δ Sharpe vs baseline | Trade reduction |
|--------|---------------------|-----------------|
| btc_only | -0.12 | -20% |
| btc_or_eth | -0.07 | -20% |
| btc_and_eth | -0.13 | -24% |

**Verdict:** Chandelier(P=7, M=2.30) already handles choppy BTC regimes correctly. Correlation filter adds no value and trades off Sharpe for trade frequency.

**Evidence file:** `examples/turtle_correlation_filter_walkforward.rs`, `snapshots/t7_correlation_filter_results.csv`
