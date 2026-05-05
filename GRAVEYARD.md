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
| Vol-contingent Chandelier | 2026-04-12 | GRAVEYARD | 21-bar vol rank: all configs identical |
| Vol-contingent Chandelier (252-bar) | 2026-04-26 | GRAVEYARD | 252-bar vol rank: tied (Sharpe Δ+0.12 noise) — no adaptive benefit |
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
| CTREND Fixed 25% Portfolio Sleeve | 2026-04-27 | REJECTED | Turtle 75/25 CTREND(EMA8/32,hold=30): DD improvement +3.6pp ✓ but Sharpe destroyed 1.38→0.33 (-76%). CTREND standalone Sharpe -2.82 makes 25% allocation too costly. DD improvement doesn't compensate. Dynamic/conditional switching untested (could be viable).
| Donchian 25% Portfolio Sleeve | 2026-04-30 | REJECTED | Base5 candidate failed 9-universe production guardrail: 34/54 pass (63%) below required 69.1%. Sharpe improved +11% vs internal Turtle baseline, but avg return dropped -164.9pp and absolute pass rate failed. Do not re-sweep nearby weights without a new mechanism. |
| Rebalancing trim_losers I=5 | 2026-04-30 | REJECTED | 9-universe S6 validation: pass improved 51/54 vs baseline 46/54 and DD improved 69.9% vs 71.6%, but Sharpe was identical (+3.828) and avg return fell +81.1% vs +87.8%. Mechanism did not create risk-adjusted edge. |
| S6 close_losers I=5 — Turtle-only | 2026-04-30 | INCOMPATIBLE | Turtle-only exit (live bot) fires in ~3-5 bars. close_losers checks at 5-bar rebalancing interval requiring >5% loss — structurally incompatible. Chandelier's longer holds (7+ bars) are what enable close_losers. Dual Chandelier+Turtle: 48/54 pass, Sharpe +3.067 improvement. Live bot Turtle-only: 0/162 (0%) — 0 trades across all universes/windows/configs. S6 is a dual-exit harness strategy only. |
| Weekend Entry Filter (T62) | 2026-05-05 | REJECTED | Skipping Sat/Sun Turtle entries worsens live-compatible WF: 58/63 → 56/63 pass, Sharpe 7.079 → 6.489, return +114.6% → +108.9%, DD 21.2% → 23.0%. Weekend bars are 28.6% of bars but 39.6% of trades — not structurally inferior. |
| Live Semantic Alignment Candidate (T69) | 2026-05-05 | REJECTED | Strict prior-window Turtle entry + VOL_LOOKBACK=92 top-3 dollar-volume gate + size-aware accounting did not close research/live gap: 1.02x / Sharpe 0.10 / MaxDD 30.8% / 200 trades vs exact live bot 2.54x / Sharpe 0.94. Do not patch bot.rs with this candidate. |

| Funding Rate Regime Filter (T57) | 2026-05-04 | NULL | BTC 3d rolling funding avg as Turtle entry gate. Dense sweep (51 values × 9 universes × 10 WF windows = 4,590 runs). Pass rate NEVER improves (67/90 at all viable thresholds). Best equity +12.5% at t=0.0008 (274 blocked entries / 2849 total). Marginal — likely noise from handful of lucky blocks. Not a robustness improvement. Distinct from GRAVEYARD'd funding rate MR (trading on funding) — this tested regime gating. Both fail. |

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

## 2026-04-29 — Asymmetric Exit Variants (T27)

**Hypothesis:** Separate catastrophic hard stop from looser profit-trailing stop: ATR×0.5 hard stop plus Chandelier(7,3.0) soft trail, or ATR×0.5 hard stop plus TurtleATR only.

**Validation:** `examples/asymmetric_exit_walkforward.rs`, 9 universes × 6 walk-forward windows, current T27 params.

**Result:** No robustness improvement. Baseline remained best: 38/54 pass, avg Sharpe 3.818. `asym_soft`: 38/54, Sharpe 3.680. `asym_hard`: 38/54, Sharpe 3.610. Asymmetric exits add complexity without improving pass rate or Sharpe.

**Verdict:** REJECTED. Keep current exit architecture for this harness.
