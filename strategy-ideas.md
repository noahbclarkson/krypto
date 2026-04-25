# Strategy Ideas — Krypto Research Log

*Last updated: 2026-04-25. Research loop closed. Only T6 (CTREND fixed-hold) and live testnet advance the project.*

---

## Top 3 Genuinely Untested Ideas

### 1. CTREND Fixed-Hold Exit Sweep (T6 — HIGHEST PRIORITY)
**Concept:** CTREND signal confirmed genuine (Monte Carlo: 0/500 shuffled beat real). But CTREND + Chandelier exit = 30/54 pass (REJECTED) — Chandelier is too tight for CTREND's slower multi-horizon timing. Test: CTREND entry + fixed-hold (10, 15, 21, 30, 45, 60, 90 bars). CTREND fires slower — it may need time to develop, not a tight trailing stop.
**Test:** `examples/ctrend_fixed_hold_walkforward.rs` — 7 hold values × 9 universes × 6 windows
**Win condition:** >35/54 pass = viable signal family (genuinely different from Turtle)
**Baseline:** Turtle+Chandelier = 43/54 pass (80%)
**Status:** UNTESTED. T6 in PLAN.md. Takes ~2 hours.

### 2. Maker-Fill Adaptive Position Sizing (Post-Live)
**Concept:** After 30 days of live testnet: measure actual maker-fill rate per symbol. If <50% → reduce position 30%. If >70% → full position. The maker-fill rate is a market microstructure signal.
**Status:** Cannot be tested without live data. FillLog infrastructure built (`src/live/executor.rs`).
**Priority:** Medium (after live validation succeeds)

### 3. Vol Regime Live Dashboard
**Concept:** Display live ATR percentile rank (vs 252-bar history) per symbol. Helps interpret drawdowns in real-time — "are we in a high-vol regime where the strategy should perform well?"
**Status:** ATR calculation exists in live bot. Missing: percentile rank + display output. Low effort (~1 hour), high interpretability.
**Priority:** Low (monitoring only)

---

## Post-Live-Testnet Concepts (For After 30-Day Validation)

These require live fill data to test. Not actionable until T9 (live testnet) completes.

### S1. Live Slippage → Position Adjustment
If SOL slippage consistently >2× model → reduce SOL cap to $25K.
Infrastructure already built: `FillLog` in `src/live/executor.rs`.

### S2. Multi-Strategy Live Sleeve (A/D as secondary)
After live validates Turtle+Chandelier: add A/D Dual-Hat as 20% sleeve for crash protection.
A/D wins crash windows historically but walk-forward pass rate is only 52% standalone.

---

## Dead Strategies — Confirmed Graveyard

| Strategy | Test Date | Result | Key Reason |
|----------|-----------|--------|------------|
| BollingerReversion | 2026-04-11 | 0/288 OOS | Signal actively harmful vs random |
| BOCPD regime detector | 2026-04-11 | 0% breaks | NIG model too insensitive |
| FDUSD basis carry | 2026-04-10 | 19% pass | Structural premium, autocorrelation 0.88 |
| Funding rate MR | 2026-04-10 | 43% pass | Highly autocorrelated |
| Vol-contingent Chandelier | 2026-04-12 | GRAVEYARD | All configs identical |
| ATR entry filter | 2026-04-13 + 2026-04-25 | mult=0.0 wins | Any non-zero filter hurts |
| Chop filter | 2026-04-13 | REJECTED | Trade-starving |
| Correlation entry filter | 2026-04-25 (T7) | REJECTED | Chandelier already handles it |
| CTREND + Chandelier exit | 2026-04-20 | 30/54 pass | Wrong exit mechanism for CTREND |
| 4h Multi-Timeframe Turtle | 2026-04-25 | 1/20 pass | Structural — dual exit collapses on 4h |
| Cross-market equity integration | 2026-04-16 | REJECTED | Combined Sharpe -2.94 vs crypto-only |
| DynamicTrend EMA signal | 2026-04-16 | REJECTED | Turtle wins 21/24 windows |
| A/D Static Sleeve | 2026-04-14 | 46% | Below-random win rate |
| BTC Trend Scalar | 2026-04-14 | 0/8 configs | Baseline wins |
| Regime-conditional allocation | 2026-04-12 | 60.5% | Worse than either component |
| XRP 4h MR | 2026-04-11 | 0/4 | Edge destroyed by fees |
| 1h Mean Reversion | 2026-04-14 | 0/6 | All symbols negative Sharpe |

**Conclusion:** The reliable crypto edge is directional trend-following on daily data. Everything else has failed.

---

## Anti-Overfitting Rules (Established 2026-04-25)

1. **Minimum win margin:** ≥3 windows (5.5%) improvement on OOS before accepting any param change
2. **No sequential optimization on same data:** If EP is optimized on data D, you cannot also optimize ATR_EM on data D and claim both are valid
3. **Held-out validation required for marginal wins:** 1-2 window delta = noise until pre-2021 stress confirms
4. **Equity curve dominance:** Winner must dominate baseline at >80% of time bars
5. **Never re-run confirmed params:** ATR_PERIOD confirmed 3×. CHAND_MULT confirmed 2×. Stop.

---

## What NOT to Research (Confirmed 2026-04-25)

| Strategy | Reason |
|----------|--------|
| EP re-optimization | EP=24 marginal (+2 windows), T3 held-out overdue |
| ATR_ENTRY_MULT | EM=0.00 definitive winner — full sweep 41 values |
| ATR_MULT re-sweep | M=2.0 confirmed, done 2× |
| Vol regime filters | All failed, Chandelier handles it |
| Position scaling | All failed — Chandelier sufficient |
| Regime switching | All failed |
| Cross-sectional momentum | Short side noise |
| 4h timeframe | Structural failure |

---

*Source of truth for production params: `src/live/config.rs`. Last verified: 2026-04-25.*
