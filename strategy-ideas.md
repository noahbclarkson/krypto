# Strategy Ideas — Krypto Research Log

*Last updated: 2026-04-26. T11 (failure mode diagnostic) and S4 (vol-adaptive Chandelier) are next priority. T9 (live testnet) blocked on Noah's API keys.*

---

## Top 3 Genuinely Untested Ideas (Priority Order)

### 1. S6: Turtle-Only Exit Test (HIGH — HIGHEST PRIORITY)
**Concept:** Turtle breakout (EP=21) with ONLY Turtle ATR(24, 2.0) exit. NO Chandelier.
**Question:** Does removing Chandelier improve performance in choppy regimes (where P=7 fires too aggressively)?
**Hypothesis:** Turtle ATR(24) is slower than Chandelier(P=7, M=2.30). In choppy regimes like 2026 YTD, Chandelier constantly stops out positions — Turtle ATR might hold through noise.
**Test:** Side-by-side walk-forward on Base5 (6 windows) + specifically on the 13 failing windows.
**Why this matters NOW:** W05 live failure is -22.7% YTD. If Turtle-only handles chop better, it's the live deployment config.
**Status:** Genuinely untested. Not in GRAVEYARD. Highest priority.

### 2. S7: Turtle + CTREND Portfolio with Current Params (MEDIUM)
**Concept:** Turtle(75%) + CTREND(EMA8/32, hold=30 bars)(25%) using CURRENT production params (CHAND_P=7, M=2.30, EP=21, HM=12, ATR=24).
**Why this matters:** T6 (2026-04-25) used STALE params (P=15, M=1.50, EP=21) and found Sharpe destroyed (1.38→0.33). Current params are significantly different (P=7 is much tighter). CTREND fixed-hold (73% pass) is genuinely uncorrelated with Turtle — different entry mechanics, different exit timing. A 2-sleeve portfolio might handle both trending AND choppy regimes better than Turtle alone.
**Note:** Even if S6 shows Turtle-only wins standalone, Turtle+CTREND might improve DD coverage.
**Status:** Untested with current production params.

### 3. S8: Donchian Entry vs Turtle Entry (MEDIUM)
**Concept:** Turtle uses `close > max(close, high) [21-bar max of either close or high]`. Donchian uses `close > highest(high) [strict high-only breakout]`.
**Why this might matter:** Donchian is the original trend-following entry (Richard Dennis, 1983). It's strictly tighter than Turtle (requires close above highest high ever, not just a 21-bar max). Might produce fewer but higher-quality signals.
**Test:** Walk-forward on Base5 (6 windows), Donchian vs Turtle, current production params.
**Status:** Never tested. Entry signal space is NOT fully explored.

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
| ATR entry filter | 2026-04-13 + 2026-04-25 | mult=0.0 wins | Any non-zero filter hurts |
| Chop filter | 2026-04-13 | REJECTED | Trade-starving |
| Correlation entry filter | 2026-04-25 (T7) | REJECTED | Chandelier already handles it |
| CTREND + Chandelier exit | 2026-04-20 | 30/54 pass | Wrong exit mechanism for CTREND |
| CTREND fixed-hold exit | 2026-04-25 (T6) | 44/60 pass | VIABLE but secondary (not standalone) |
| 4h Multi-Timeframe Turtle | 2026-04-25 | 1/20 pass | Structural — dual exit collapses on 4h |
| Cross-market equity integration | 2026-04-16 | REJECTED | Combined Sharpe -2.94 vs crypto-only |
| DynamicTrend EMA signal | 2026-04-16 | REJECTED | Turtle wins 21/24 windows |
| A/D Static Sleeve | 2026-04-14 | 46% | Below-random win rate |
| BTC Trend Scalar | 2026-04-14 | 0/8 configs | Baseline wins |
| Regime-conditional allocation | 2026-04-12 | 60.5% | Worse than either component alone |
| XRP 4h MR | 2026-04-11 | 0/4 | Edge destroyed by fees |
| 1h Mean Reversion | 2026-04-14 | 0/6 | All symbols negative Sharpe |

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
- ATR_ENTRY_MULT=0.85 REJECTED: same violation (2026-04-25)
- P=7 VALIDATED: held-out confirmed with EP=21 (2026-04-26 T3-Next: 27/29 vs 27/29, delta +0.01 Sharpe)
- EP=21 VALIDATED: held-out confirmed (2026-04-26 T3: 27/29 pass)

---

## What NOT to Research (Confirmed 2026-04-26)

| Strategy | Reason |
|----------|--------|
| EP re-optimization | ✅ EP=21 confirmed. EP=24 was in-sample inflation. |
| ATR_ENTRY_MULT | EM=0.00 definitive winner — full 41-value sweep |
| ATR_MULT re-sweep | M=2.0 confirmed, done 2× |
| ATR_PERIOD re-sweep | ATR=24 confirmed 3× — null result 2026-04-25 |
| CHAND_PERIOD re-sweep | P=7 confirmed vs P=11 on held-out (EP=21) |
| Vol regime filters | All failed, Chandelier handles it |
| Position scaling | All failed — Chandelier sufficient |
| Regime switching | All failed |
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

*Source of truth for production params: `src/live/config.rs`. Last verified: 2026-04-26.*