# Strategy Ideas — Krypto Research Log

*Last updated: 2026-04-27. T18 (S17 deploy/revert) is top priority. S8 Donchian genuinely untested. Live testnet BLOCKED on Noah's API keys.*

---

## Top 3 Genuinely Untested Ideas (Priority Order)

### 1. T18: S17 Conclusion — Deploy or Revert (CRITICAL)
**Concept:** S17 walk-forward (2026-04-27) proved: with production params EP=21/CHAND_P=7/M=2.30, Chandelier fires first in 336/336 trades (100%) and degrades Sharpe by -1.47 vs Turtle-only. But the POSITION_CAP commit (`049b13ed`, 20 min later) reverted the bot code back to `check_dual_exit`. The S17 conclusion is NOT in HEAD.
**Two paths:**
- **Deploy Turtle-only:** Actually remove Chandelier from `src/live/bot.rs`. Then re-run POSITION_CAP under Turtle-only to confirm CAP=3 still holds.
- **Keep Chandelier:** Document why Chandelier is kept despite S17 showing it degrades Sharpe. The CAP=3 validation (under dual-exit) would then be applicable.
**Status:** CRITICAL. Decision required before any further work.

### 2. S8: Donchian Entry vs Turtle Entry (HIGH — GENUINELY UNTESTED)
**Concept:** Turtle uses `close > max(close, high) [21-bar max of either].` Donchian uses `close > highest(high) [strictest — all-time high breakout].` Donchian is the original Richard Dennis 1983 Turtle system entry.
**Why this might matter:** Entry signal space is almost completely unexplored. All our work has been on exit optimization. Donchian might produce fewer but higher-quality signals. Test with Turtle ATR(24, 2.0) as sole exit.
**Test:** Walk-forward on Base5 (6 windows), Donchian vs Turtle entry, Turtle ATR(24, 2.0) sole exit.
**Status:** Never tested. No harness exists. PLAN.md falsely marked this as DONE — it was never executed.

### 3. Regime-Adaptive EP Switch (NEW — genuinely untested)
**Concept:** Not CTREND switching (failed). Not strategy switching. Instead: if 21-bar ATR percentile rank > 70th (high vol = trending), use EP=21. If < 30th (low vol = choppy), widen entry to EP=28 or switch to Donchian entry. This adapts the entry threshold, not the strategy.
**Why this matters:** 10+ approaches to fix the chop problem failed (overlays, regime switching, position scaling). Adapting the entry threshold is a fundamentally different mechanism — it changes what constitutes a valid breakout signal based on vol regime.
**Different from all failed approaches:** All failed approaches tried to modify/exit/overlay Turtle+Chandelier. This changes the entry condition itself.
**Status:** Never tested. Genuinely novel. Depends on S8 (need Donchian harness first).

---

## Post-Live-Testnet Concepts (Cannot be backtested)

### S1. Maker-Fill Adaptive Position Sizing
**Concept:** After 30 days of live testnet: measure actual maker-fill rate per symbol.
- If <50% → reduce position 30% (execution degraded)
- If >70% → full position (execution optimal)
The maker-fill rate is a market microstructure signal.
**Status:** Cannot be backtested. FillLog infrastructure built in `src/live/executor.rs`.

### S2. Vol Regime Live Dashboard
**Concept:** Display live ATR percentile rank (vs 252-bar history) per symbol. Helps interpret drawdowns in real-time.
**Status:** ATR calculation exists. Missing: percentile rank + display output.

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
| ATR entry filter | 2026-04-13 + 04-25 | mult=0.0 wins | Any non-zero filter hurts |
| Chop filter | 2026-04-13 | REJECTED | Trade-starving |
| Correlation entry filter | 2026-04-25 (T7) | REJECTED | Chandelier already handles it |
| CTREND + Chandelier exit | 2026-04-20 | 30/54 pass | Wrong exit mechanism for CTREND |
| CTREND fixed-hold exit | 2026-04-25 (T6) | 44/60 pass | VIABLE but secondary (not standalone) |
| **CTREND regime-conditional switching** | **2026-04-25 (T13)** | **67% pass — FAIL** | **67% < 70% threshold. NOT best candidate.** |
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

*Source of truth for production params: `src/live/config.rs`. Last verified: 2026-04-27.*
