# Strategy Ideas — Krypto Research Log

*Last updated: 2026-04-28. Critique cycle: T22 elevated to #1. Entry space CLOSED. Live testnet BLOCKED on Noah's API keys — nothing else matters.*

---

## Critical New Insight: Entry Space Is a Trade-Off, Not a New Edge

**Donchian result (T19, 2026-04-28) changes the picture:**

- Donchian: avg Sharpe +9.4, pass rate 86% (-14pp vs Turtle)
- Turtle: avg Sharpe +5.6, pass rate 100%
- **Entry alternatives trade pass rate for per-trade Sharpe quality.** They don't add new edge — they filter signals.

This pattern matches every failed entry approach:
- ATR_MULT (fixed threshold): pass rate degrades monotonically as threshold increases
- Volume confirmation: pass rate degrades 6-13pp
- Correlation filter: loses to baseline on every metric

**Implication:** ATR-rank conditional filter (T23) is the last untested entry idea. If it also trades pass rate for Sharpe, the entry space is definitively exhausted. Turtle entry is the optimal trade-off between signal frequency and signal quality.

---

## Top 4 Genuinely Untested Ideas (Priority Order)

*(Updated 2026-04-28 19:40 — added vol-norm position sizing as speculative #4)*

### NEW: T22 — Dual-Exit Attribution (CRITICAL — most important untested question)
**Concept:** At CHAND_P=7/M=2.30, Chandelier fires at ~bar 7-12. Instrument walk-forward to track which exit fires first — Chandelier or Turtle ATR — per trade and per window.
**Why this matters (updated 2026-04-28):** If Chandelier fires first >90% of trades, TURTLE_ATR_PERIOD=24 and TURTLE_ATR_MULT=2.0 are non-binding parameters validated on noise. The strategy is effectively Chandelier(7,2.30) + safety net. Every dual-exit hyperopt result needs reinterpretation.
**This is the most important structural test remaining.** Run it before T21.
**Status:** MUST RUN.
**Concept:** Re-run POSITION_CAP sweep under actual production dual-exit logic (Chandelier(7,2.30) + Turtle ATR(24,2.0)), not Turtle-only harness.
**Why this matters:** CAP=3 was validated on Turtle-only exit harness. Production bot uses dual-exit. CAP=3 "likely holds" but is a structural gap between validated harness and production code.
**Risk if skipped:** Live testnet goes live with position sizing validated on the wrong exit logic.
**Status:** Critical gap. No harness exists for dual-exit CAP sweep.

### T22: Turtle-Only vs Dual-Exit Attribution (MEDIUM — structural understanding)
**Concept:** Instrument the walk-forward harness to count Chandelier-first vs Turtle ATR-first exits per window.
**Why this matters:** At P=7/M=2.30, Chandelier is extremely tight (fires at ~bar 7-12). If it fires first >90% of trades, dual-exit ≈ Turtle-only + safety net. If Turtle ATR fires first >50%, dual-exit adds genuine marginal value.
**Decision value:** Determines whether we're actually testing what we think we're testing.
**Status:** Structural understanding, not new strategy. No attribution harness exists.

### T23: ATR-Rank Conditional Entry Filter (MEDIUM — last untested idea)
**Concept:** Not a fixed ATR_MULT (failed at all values). Instead: only enter if current 21-bar ATR is above its Nth percentile in 252-bar history. High ATR = trending = valid Turtle setup. Low ATR = choppy = filter out.
**Why this matters:** ATR_MULT (fixed threshold) failed at all values. ATR-rank is regime-dependent — mechanically different. **Caution:** Donchian result shows entry alternatives trade pass rate for per-trade Sharpe. ATR-rank may have the same problem.
**Test:** Sweep threshold {50th, 60th, 70th percentile} × Base5 × 6 windows.
**Status:** Last genuinely untested idea. If it fails, entry space is definitively closed.

---

## Live Testnet Blocker

**Status: CRITICAL BLOCKER — nothing else advances without this.**

The entire project is simulation. All metrics are upper bounds:
- Walk-forward Sharpe 5.46 (Base5) — upper bound, per-window averaged methodology
- **Equity Sharpe ~1.29 — honest number, from compounded daily equity curve**
- $10K→$67M — simulation maximum
- Pre-2021 stress: 67.9% — BELOW our own 70% threshold (most honest metric)

What live testnet validates:
1. Maker-fill rate: is 70% estimate accurate in live conditions?
2. Slippage model: does actual execution match our 5bps assumption?
3. Strategy execution: does the bot actually run without errors?
4. Real-time data: does Binance WebSocket feed work correctly?

**Escalation: BLOCKED for 3+ weeks. Every session identifies this as critical.**

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

### S4. Volatility-Normalized Position Sizing (Speculative — can be backtested)
**Concept:** Equal $ exposure per symbol (ATR-normalized notional) vs equal capital allocation.
- Current: allocate equal capital to each of up to CAP=3 symbols
- Proposed: allocate equal ATR-normalized notional (e.g., $10K / 21-bar ATR each)
- Mechanism: high-vol symbols naturally get smaller positions, low-vol symbols get larger — Kelly-style position sizing
- **Different from failed CAP scaling overlays:** those changed the CAP scalar itself. This keeps CAP=3 but adjusts per-symbol notional within the cap.
- **Risk:** may reduce return in trending windows (underweights high-vol breakouts that work). Only test to know.
- Test: 3 configs {equal_capital, atr_norm_10k, atr_norm_20k} × Base5 × 6 windows.
- **Note:** This is testable NOW, does not require live testnet.

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
| Correlation entry filter (T7) | 2026-04-25 | REJECTED | All 3 variants lose to baseline |
| CTREND + Chandelier exit | 2026-04-20 | 30/54 pass | Wrong exit mechanism for CTREND |
| CTREND fixed-hold exit | 2026-04-25 | 44/60 pass | VIABLE but secondary (not standalone) |
| CTREND regime-conditional switching | 2026-04-25 | 67% pass — FAIL | 67% < 70% threshold |
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
| Donchian entry | 2026-04-28 | REJECTED | Wins Sharpe (+3.8) but loses pass rate (-14pp) — not a replacement |
| ATR-rank conditional filter | UNTESTED | T23 | Last untested idea — may have same pass-rate trade-off |

**Conclusion:** The reliable crypto edge is directional trend-following on daily data. Everything else has failed or is secondary. Entry space is a pass-rate vs Sharpe trade-off — Turtle is likely the optimal point.

---

## Anti-Overfitting Rules (Established 2026-04-25, Applied 2026-04-26)

1. **Minimum win margin:** ≥3 windows (5.5%) improvement on OOS before accepting any param change
2. **No sequential optimization on same data:** If EP is optimized on data D, you cannot also optimize ATR_EM on data D and claim both are valid
3. **Held-out validation required for marginal wins:** 1-2 window delta = noise until pre-2021 stress confirms
4. **Equity curve dominance:** Winner must dominate baseline at >80% of time bars
5. **Never re-run confirmed params:** ATR_PERIOD confirmed 3×. CHAND_MULT confirmed 2×. P=7 confirmed 2×. Stop.

**Application history:**
- EP=24 REJECTED: in-sample inflation on same OOS data as P=7 and ATR_ENTRY_MULT
- EP=43 REJECTED: same violation (found same session as EP=21 validation)
- ATR_ENTRY_MULT=0.85 REJECTED: same violation
- P=7 VALIDATED: held-out confirmed with EP=21 (2026-04-26 T3-Next: 27/29 vs 27/29, delta +0.01 Sharpe)
- EP=21 VALIDATED: held-out confirmed (2026-04-26 T3: 27/29 pass)

---

## Key Insight: All Metrics Are Upper Bounds

Everything in HALL_OF_FAME.md is a simulation maximum. The real validation path is:
1. Run live testnet paper trading
2. Compare actual vs predicted metrics (maker fill rate, slippage, Sharpe)
3. Calibrate the execution model based on real feedback
4. Only then can we say whether the strategy is genuinely robust

**Source of truth for production params: `src/live/config.rs`. Last verified: 2026-04-28.**
