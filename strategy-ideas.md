# Strategy Ideas — Krypto Research Log

*Last updated: 2026-04-27. T19 (Donchian harness) is #1 priority. T20 (ATR-rank filter) is #2. Live testnet BLOCKED on Noah's API keys — nothing else matters.*

---

## Top 3 Genuinely Untested Ideas (Priority Order)

### 1. T19: Donchian Entry Walk-Forward (HIGH — genuinely untested, #1 priority)
**Concept:** Turtle uses `close > max(close, high) [21-bar]`. Donchian uses `close > highest(high) [strictest — all-time high breakout]`. The original 1983 Richard Dennis Turtle system used Donchian entry. Fewer signals, potentially higher quality.
**Why this matters:** All our work has been on exit optimization. Entry signal space is almost completely unexplored. Every strategy comparison held entry constant and varied exit. Donchian tests an alternative entry hypothesis — whether a stricter breakout threshold (all-time high vs 21-bar high of close/high) produces better risk-adjusted returns.
**Test:** Walk-forward on Base5 (6 windows), Donchian entry vs Turtle entry, Turtle ATR(24, 2.0) as sole exit.
**Status:** Never tested. No harness exists. No chart. No snapshots. PLAN.md falsely marked this as DONE.
**Execution:** Build `examples/donchian_walkforward.rs`, run 6-window Base5 walk-forward, compare to Turtle baseline.

### 2. T20: ATR-Rank Conditional Entry Filter (MEDIUM — genuinely untested, #2 priority)
**Concept:** Not a fixed ATR_MULT (failed at all values). Instead: only enter if current 21-bar ATR is above its 60th percentile in 252-bar history. High ATR = trending environment = valid Turtle setup. Low ATR = choppy = filter out.
**Why this matters:** ATR_MULT is a fixed threshold — it treats all market regimes the same. ATR-rank is regime-dependent: in high-vol regimes (which trend), the threshold is automatically higher; in low-vol chop, it's automatically stricter. This is mechanistically different from every failed approach which tried overlays/exits/position sizing rather than entry quality control.
**Test:** Sweep threshold {50th, 60th, 70th percentile} × Base5 6-window walk-forward. Compare pass rate and Sharpe to baseline (no filter).
**Status:** Never tested. Genuinely novel. No prior art in our codebase.
**Execution:** Build harness, run sweep. If 60th beats baseline, test 55th and 65th for fine calibration.

### 3. Regime-Adaptive EP Switch (NEW — genuinely untested)
**Concept:** If 21-bar ATR percentile rank > 70th (trending) → EP=21. If < 30th (choppy) → EP=28 or Donchian entry. Adapts entry threshold based on vol regime, not strategy classifier.
**Why this matters:** 10+ approaches to fix the chop problem failed (overlays, regime switching, position scaling). All tried to modify/exit/overlay Turtle+Chandelier. This changes the entry condition itself — a fundamentally different mechanism.
**Different from all failed approaches:** CTREND regime switching (67% fail), Vol-contingent Chandelier (identical results), position scaling (all failed). This adapts entry threshold, not strategy/exit/position.
**Status:** Never tested. Depends on T19 results (need Donchian harness first).
**Priority:** LOW for now — build T19 first, then T20, then assess whether regime-adaptive EP is needed.

---

## Live Testnet Blocker

**Status: CRITICAL BLOCKER — nothing else advances without this.**

The entire project is simulation. All metrics are upper bounds:
- Walk-forward Sharpe 5.46 (Base5) — upper bound
- Equity Sharpe ~1.29 — upper bound
- $10K→$67M — simulation maximum

We need:
- Binance testnet API key + secret (NOT production keys)
- Testnet endpoint configured in `src/live/executor.rs`

What live testnet validates:
1. Maker-fill rate: is 70% estimate accurate in live conditions?
2. Slippage model: does actual execution match our 5bps assumption?
3. Strategy execution: does the bot actually run without errors?
4. Real-time data: does Binance WebSocket feed work correctly?

**Escalation: This has been blocked for weeks. Every session identifies this as critical. Nothing advances the project until Noah provides testnet keys.**

---

## Post-Live-Testnet Concepts (Cannot be backtested)

### S1. Maker-Fill Adaptive Position Sizing
After 30 days of live testnet: measure actual maker-fill rate per symbol.
- If <50% → reduce position 30% (execution degraded)
- If >70% → full position (execution optimal)
The maker-fill rate is a market microstructure signal.

### S2. Vol Regime Live Dashboard
Display live ATR percentile rank (vs 252-bar history) per symbol. Helps interpret drawdowns in real-time.

### S3. Live Slippage → Position Adjustment
If SOL slippage consistently >2× model → reduce SOL cap to $25K notional.

---

## Dead Strategies — Confirmed Graveyard

| Strategy | Test Date | Result | Key Reason |
|----------|-----------|--------|------------|
| BollingerReversion | 2026-04-11 | 0/288 OOS | Signal actively harmful vs random |
| BOCPD regime detector | 2026-04-11 | 0% breaks | NIG model too insensitive |
| FDUSD basis carry | 2026-04-10 | 19% pass | Structural premium, autocorrelation 0.88 |
| Funding rate MR | 2026-04-10 | 43% pass | Highly autocorrelated |
| Vol-contingent Chandelier | 2026-04-12 | GRAVEYARD | All configs identical |
| ATR entry filter (fixed mult) | 2026-04-13 + 04-25 | mult=0.0 wins | Any non-zero filter hurts |
| Chop filter | 2026-04-13 | REJECTED | Trade-starving |
| Correlation entry filter | 2026-04-25 | REJECTED | Chandelier already handles it |
| CTREND + Chandelier exit | 2026-04-20 | 30/54 pass | Wrong exit mechanism for CTREND |
| CTREND fixed-hold exit | 2026-04-25 | 44/60 pass | VIABLE but secondary (not standalone) |
| CTREND regime-conditional switching | 2026-04-25 | 67% pass — FAIL | 67% < 70% threshold. NOT best candidate. |
| 4h Multi-Timeframe Turtle | 2026-04-25 | 1/20 pass | Structural — dual exit collapses on 4h |
| Cross-market equity integration | 2026-04-16 | REJECTED | Combined Sharpe -2.94 vs crypto-only |
| DynamicTrend EMA signal | 2026-04-16 | REJECTED | Turtle wins 21/24 windows |
| A/D Static Sleeve | 2026-04-14 | 46% | Below-random win rate |
| BTC Trend Scalar | 2026-04-14 | 0/8 configs | Baseline wins |
| Regime-conditional allocation | 2026-04-12 | 60.5% | Worse than either component alone |
| XRP 4h MR | 2026-04-11 | 0/4 | Edge destroyed by fees |
| 1h Mean Reversion | 2026-04-14 | 0/6 | All symbols negative Sharpe |
| EP=24 | 2026-04-26 | REVERTED | In-sample inflation on same OOS data |
| EP=43 | 2026-04-27 | REVERTED | Found same session as EP=21 validation |
| ATR_ENTRY_MULT=0.85 | 2026-04-25 | REVERTED | In-sample inflation, same session as EP=24/P=7 |
| Position scaling overlays | various | GRAVEYARD | All failed — Chandelier already handles it |
| Donchian entry | UNTESTED | S8 | Never built harness |
| ATR-rank conditional filter | UNTESTED | T20 | Never built harness |

**Conclusion:** The reliable crypto edge is directional trend-following on daily data. Everything else has failed or is secondary.

---

## Anti-Overfitting Rules (Established 2026-04-25, Applied 2026-04-26)

1. **Minimum win margin:** ≥3 windows (5.5%) improvement on OOS before accepting any param change
2. **No sequential optimization on same data:** If EP is optimized on data D, you cannot also optimize ATR_EM on data D and claim both are valid
3. **Held-out validation required for marginal wins:** 1-2 window delta = noise until pre-2021 stress confirms
4. **Equity curve dominance:** Winner must dominate baseline at >80% of time bars
5. **Never re-run confirmed params:** ATR_PERIOD confirmed 3×. CHAND_MULT confirmed 2×. P=7 confirmed 2×. Stop.

**Application history:**
- EP=24 REJECTED: in-sample inflation on same OOS data as P=7 and ATR_ENTRY_MULT (2026-04-26)
- EP=43 REJECTED: same violation (found same session as EP=21 validation, 2026-04-27)
- ATR_ENTRY_MULT=0.85 REJECTED: same violation (2026-04-25)
- P=7 VALIDATED: held-out confirmed with EP=21 (2026-04-26 T3-Next: 27/29 vs 27/29, delta +0.01 Sharpe)
- EP=21 VALIDATED: held-out confirmed (2026-04-26 T3: 27/29 pass)

---

## What NOT to Research (Confirmed 2026-04-27)

| Strategy | Reason |
|----------|--------|
| EP re-optimization | ✅ EP=21 confirmed. EP=24 and EP=43 both rejected as in-sample inflation. |
| ATR_ENTRY_MULT | EM=0.00 definitive winner — full 41-value sweep |
| ATR_MULT re-sweep | M=2.0 confirmed, done 2× |
| ATR_PERIOD re-sweep | ATR=24 confirmed 3× — null result 2026-04-25 |
| CHAND_PERIOD re-sweep | P=7 confirmed vs P=11 on held-out (EP=21) |
| Vol regime filters | All failed, Chandelier handles it |
| Position scaling | All failed — Chandelier sufficient |
| CTREND regime-conditional switching | FAILED (67% pass < 70% threshold) |
| Cross-sectional momentum | Short side noise |
| 4h timeframe | Structural failure |

---

## CTREND Fixed-Hold Result (T6 — 2026-04-25)

**Signal:** Monte Carlo confirmed genuine (0/500 shuffled beat real).
**Test:** CTREND entry (EMA8>EMA32 crossover) + fixed-hold exits (10, 15, 21, 30, 45, 60, 90 bars).
**Result:** 44/60 pass (73%) — win condition met (>35/54 pass).
**Winner:** hold=30 bars (best Sharpe + pass rate balance).
**Role:** Secondary signal family, NOT a standalone replacement. Expected role: 20-30% portfolio sleeve.
**Baseline comparison:** Turtle+Chandelier = 80%+ pass rate. CTREND fixed-hold is weaker standalone.

---

## Key Insight: All Metrics Are Upper Bounds

Everything in HALL_OF_FAME.md is a simulation maximum. The real validation path is:
1. Run live testnet paper trading
2. Compare actual vs predicted metrics (maker fill rate, slippage, Sharpe)
3. Calibrate the execution model based on real feedback
4. Only then can we say whether the strategy is genuinely robust

**Source of truth for production params: `src/live/config.rs`. Last verified: 2026-04-27.**