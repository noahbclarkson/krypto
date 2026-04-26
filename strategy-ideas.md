# Strategy Ideas — Krypto Research Log

*Last updated: 2026-04-26. T3 + T3-Next complete. All production params validated. New priority: T11 failure mode diagnostic.*

---

## Top 3 Genuinely Untested Ideas

### 1. T11: Failure Mode Diagnostic (HIGH — NOT STARTED)
**Concept:** 13/54 global walk-forward windows fail (24%). We know LTC/EOS/BCH are primary culprits but have never classified HOW MANY are purely asset-specific vs. regime-specific failures.

**Question:** In how many failing windows do BTC/ETH/SOL also fail? If regime-wide failures exist (2+ production-universe symbols fail), Base5's 100% pass rate may be partially bull-era survivorship bias.

**Test:** For each of the 13 failing windows, report per-symbol pass/fail. Classify as:
- Asset-specific: only LTC/EOS/BCH fail, BTC/ETH/SOL/DOGE/XRP pass → clean for production
- Regime-wide: 2+ production symbols fail → potential tail risk in live deployment

**Decision:** If ≥3 regime-wide failures found, CTREND sleeve becomes urgent portfolio protection. If all 13 are asset-specific, production universe is clean and T6-NEXT is optional diversification.

**Status:** Not started.

### 2. T6-NEXT: CTREND Fixed-Hold Portfolio Sleeve Test (MEDIUM — NOT STARTED)
**Result (2026-04-25):** CTREND fixed-hold (hold=30 bars) = 44/60 pass (73%) — genuine signal, weaker than Turtle (80%+). Win condition met (>35/54) but CTREND is NOT a standalone replacement.

**Concept:** Test CTREND fixed-hold (hold=30) as 20-30% portfolio sleeve alongside Turtle+Chandelier (70-80%). Walk-forward comparing Turtle-only vs Turtle+CTREND sleeve.

**Win condition:** Adding CTREND sleeve reduces MaxDD by >3pp without reducing Sharpe by >10%.

**Why:** Single-strategy risk. Turtle+Chandelier is the only validated strategy. CTREND fixed-hold is the only untested idea that produces a genuinely different signal family (not just parameter tuning). Different regime sensitivity provides genuine diversification.

**Status:** T6 complete (signal confirmed). T6-NEXT not started.

### 3. T10: HOF Generation Script (MEDIUM — NOT STARTED)
**Concept:** Build `scripts/generate_hall_of_fame.rs` — parse `src/live/config.rs` + `examples/live_turtle_chandelier.rs` → auto-generate HALL_OF_FAME.md from code.

**Why:** HOF has been manually updated and contradictory 5+ times. Source of truth should be code, not markdown. Critical hygiene before live testnet deployment.

**Status:** Not started.

---

## Post-Live-Testnet Concepts

These require live fill data to test. Not actionable until T9 (live testnet) completes.

### S1. Maker-Fill Adaptive Position Sizing
**Concept:** After 30 days of live testnet: measure actual maker-fill rate per symbol. If <50% → reduce position 30%. If >70% → full position. The maker-fill rate is a market microstructure signal.
**Status:** Cannot be backtested. FillLog infrastructure built (`src/live/executor.rs`).

### S2. Vol Regime Live Dashboard
**Concept:** Display live ATR percentile rank (vs 252-bar history) per symbol. Helps interpret drawdowns in real-time.
**Status:** ATR calculation exists. Missing: percentile rank + display output.

### S3. Live Slippage → Position Adjustment
If SOL slippage consistently >2× model → reduce SOL cap to $25K.

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