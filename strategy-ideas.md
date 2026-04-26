# Strategy Ideas — Krypto Research Log

*Last updated: 2026-04-26. T11 (failure mode diagnostic) and S4 (vol-adaptive Chandelier) are next priority. T9 (live testnet) blocked on Noah's API keys.*

---

## Top 3 Genuinely Untested Ideas (Priority Order)

### 1. T11: Failure Mode Diagnostic (HIGH — NOT STARTED)
**Concept:** 13/54 global walk-forward windows fail (24%). We know LTC/EOS/BCH are primary culprits but have never classified HOW MANY are purely asset-specific vs. regime-specific failures.

**Question:** In how many failing windows do BTC/ETH/SOL also fail? If regime-wide failures exist (2+ production-universe symbols fail), Base5's 100% pass rate may be partially bull-era survivorship bias.

**Test:** For each of the 13 failing windows, report per-symbol pass/fail. Classify as:
- Asset-specific: only LTC/EOS/BCH fail, BTC/ETH/SOL/DOGE/XRP pass → clean for production
- Regime-wide: 2+ production symbols fail → potential tail risk in live deployment

**Decision:** If ≥3 regime-wide failures found, CTREND sleeve becomes urgent portfolio protection. If all 13 are asset-specific, production universe is clean.

**Why this matters NOW:** 2026 YTD live performance (-22.7% vs BTC +12.7%) is a W05-equivalent regime. Classifying the historical failures tells us if the live failure is structural or just bad luck.

**Status:** Not started. Highest priority.

### 2. S4: Vol-Adaptive Chandelier (Structural Rethink — MEDIUM)
**Concept:** Current Chandelier(P=7, M=2.30) is static. In choppy high-vol regimes (like 2026 YTD), the tight multiplier fires constantly causing whipsaw losses.

**Idea:** Use 252-bar realized vol rank (matching our ATR baseline):
- Vol > 75th percentile of 252-bar history → M=3.0+ (wider stop, holds through noise)
- Vol < 25th percentile → M=1.75 (tighter stop)
- Middle range → M=2.30 (production default)

**Why this might work when previous attempt failed:**
The prior vol-contingent test (2026-04-12) used 21-bar realized vol rank. This is too fast-moving — it doesn't capture the multi-month vol regime shifts that cause W05-style chop. 252-bar vol rank is the standard for ATR calculations and matches our regime detection baseline.

**Test:** Chandelier P=7, M ∈ {1.75, 2.30, 3.00} conditional on 252-bar vol percentile. Walk-forward on Base5 (6 windows).

**Win condition:** Vol-adaptive M reduces MaxDD by >3pp in failing windows without reducing Sharpe in passing windows.

**Status:** Not started.

### 3. S5: Turtle Entry Only, No Chandelier (MEDIUM — NOT STARTED)
**Concept:** Test Turtle breakout (EP=21) with ONLY Turtle ATR(24, 2.0) exit — no Chandelier. Single-exit vs dual-exit.

**Hypothesis:** In choppy high-vol regimes, Chandelier(P=7, M=2.30) fires too early. Turtle ATR(24) is slower and might hold positions through noise that Chandelier would stop out.

**Test:** Side-by-side comparison of:
- Turtle only: EP=21, exit=Turtle ATR(24, 2.0), no Chandelier
- Turtle+Chandelier (production): EP=21, exit=dual (Chandelier OR Turtle ATR fires first)

Walk-forward on Base5 (6 windows) + failing windows specifically (T11 results).

**Risk:** Chandelier is what makes the strategy work in trending markets. Removing it might break the edge entirely. This is a hedge against W05-style chop, not a replacement for the production strategy.

**Status:** Not started. Depends on T11 results.

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