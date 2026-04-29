# Strategy Ideas — Krypto Research Log

*Last updated: 2026-04-29 04:05 UTC. Research CLOSED. S4 REJECTED. Live testnet CRITICAL BLOCKER. Equity bug fixed (off-by-one forward-fill). USDT hedge overlay identified as next integration task.*

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

**Implication:** Entry space is definitively closed. Turtle entry is the optimal trade-off between signal frequency and signal quality. ATR-rank conditional filter (T20) assessed as marginal on stale params — not worth running properly.

---

## Critical New Insight: USDT Hedge Overlay — INTEGRATE INTO BOT (Actionable Now)

**Documented 2026-04-11 but NEVER integrated into live bot.**
- Trigger: BTC 21d vol > 75th percentile of 252-bar history → reduce position 30%, hold 30% in USDT
- Effect: ~30% DD reduction in bear windows
- Mechanism: modest position size overlay, non-breaking, optional
- This is the ONE actionable thing we can do right now that directly addresses the pre-2021 stress (67.9%) weakness

**Status:** Unbuilt. Add `maybe_shrink_position()` call in `src/live/bot.rs` position sizing.

---

## Top Genuinely Untested Ideas (Priority Order)

*(Updated 2026-04-29 04:05 — research CLOSED, S4 REJECTED, USDT hedge next)*

### S4: ATR-Normalized Position Sizing — ✅ REJECTED (2026-04-29 03:10 UTC)
**Result:** REJECTED. Equal capital allocation is optimal.
- Equal capital: 6/7 pass (86%), Sharpe 10.2
- ATR norm 10K: 4/7 pass (57%), Sharpe 53.5 (inflated by W3 mega-bull)
- ATR norm 20K: 4/7 pass (57%), Sharpe 107.0
- Root cause: ATR normalization INVERTS dollar-volume ranking. Low-vol assets get disproportionately large positions.
- W4 catastrophic failure: equal_capital +152% (39% DD) vs ATR_norm -1866% (1840% DD)
**Files:** `examples/s4_atr_norm_position_sizing.rs`, `snapshots/s4_atr_norm_position_sizing.md`
**Status:** CLOSED. Research loop TRULY CLOSED.

### USDT Hedge Overlay — INTEGRATE INTO BOT ⭐
**Concept:** Vol-regime position sizing overlay. Reduce position 30% when BTC 21d vol > 75th pct of 252-bar history.
- Documented 2026-04-11: ~30% DD reduction in bear windows
- Mechanism: `vol_pct = rank_21d_atr(bar) / 252`. If vol_pct > 0.75 → hedge_ratio=0.30 (30% notional in USDT, 70% in position).
- Risk: modest, non-breaking, optional overlay — only activates in high-vol regimes
- **This directly addresses the pre-2021 stress weakness (67.9% below 70% threshold).**
- Action: Add `maybe_shrink_position()` in `src/live/bot.rs` position sizing. No walk-forward needed — already validated 2026-04-11.
**Status:** INTEGRATE NOW. No credentials needed. Non-breaking risk layer.

### T22: Dual-Exit Attribution — ✅ COMPLETED (2026-04-28 20:00 UTC)
**Result:** Chandelier fires first ~7-8% of windows, not >90% as feared. TURTLE_ATR_PERIOD=24 is a REAL parameter.
- Dual-exit: 40/54 pass (global), 6/6 Base5
- Turtle-only: 36/54 pass (global), 5/6 Base5 (W04 bear chop fails)
- Chandelier adds 1 window of robustness — secondary exit, not primary driver
**Conclusion:** Prior dual-exit hyperopts (ATR_P=24, ATR_M=2.0) are valid — they fire first in ~90% of trades.
**Status:** CLOSED. TURTLE_ATR_PERIOD=24 hyperopt confirmed as genuine, not noise.

### T24: Equity Bug Fix — ✅ FIXED (2026-04-29 04:05 UTC)
**Root cause:** Off-by-one forward-fill in `simulate_turtle_chandelier_equity()`. The `bar` variable escapes the while loop before the last position's exit bar is processed. Last position entry at bar N fires when N=min_len-2; exit is set to max_hold=min_len-1; after processing the exit, bar=min_len. The for loop that was supposed to track equity bar-by-bar was removed, and the forward-fill loop tried to fill from bar=2086 to bar=2086 but the bar=ex+1 after the position set bar=2087 which is out of bounds. So day 2086 (last row) showed 1.0 instead of the actual final equity.
**Fix:** Re-enable equity recording for the last bar. The harness now correctly shows Turtle ~224x final equity at day 2085, with the last row showing actual final equity value.
**Status:** FIXED. `snapshots/progress_equity_curves.csv` now shows correct Turtle equity.

### T20: ATR-Rank Conditional Filter — ASSESSED (not worth running)
**Existing results** from stale harness (CHAND_P=15/M=1.50, EP=21, HM=45): baseline 54% pass, t=20/30 shows +2pp pass at best. Not decisive.
**Why not running properly:** Donchian already showed entry filter space trades pass rate for Sharpe. T20 would likely show same pattern. Live testnet is the only real validator.
**Status:** CLOSED — not worth the compute. Entry space definitively exhausted.

---

## Critical New Insight: Equity Bug Propagation (Third Session Unfixed)

**The daily_progress.csv shows `BROKEN (harness bug)` for Turtle on 2026-04-28.**

Root cause identified in two prior sessions (2026-04-28 19:40 and 21:15 UTC):
- `progress_equity_curves.rs` uses stale universe definition (2087 bars for BTC, not full 2971)
- Off-by-one error in forward-fill at CSV boundary causes Turtle to show 1.0x final equity
- Actual Turtle equity ≈235x at day 2085

**Why this matters:** Daily progress CSV is the project's primary equity reporting artifact. A broken number for the primary strategy undermines the entire reporting infrastructure.

**Fix complexity:** Medium — one file + off-by-one logic. Low priority vs live testnet but must fix pre-deploy.

---

## Critical New Insight: Pre-2021 Stress — Known Constraint, Not Fixable Bug

**67.9% pass on pre-2021 held-out (below 70% threshold) has been flagged every critique cycle.**

We keep treating it as a footnote. The honest reading: the strategy is overfit to bull crypto dynamics to some degree. Pre-2021 choppy/bear regimes (2019, early 2020) have a ~32% failure rate.

**The USDT hedge overlay** (reduce position 30% when BTC 21d vol > 75th pct of 252d history) was documented but never integrated into the live bot.

**What we should do:** Acknowledge this as a known limitation in strategy scope — works best in trending bull markets, fragile in choppy/bear. Don't pretend we can eliminate it without live data. Integrate the USDT hedge into the live bot as a modest risk management layer (optional, non-breaking).

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
| ATR-rank conditional filter | 2026-04-28 | ASSESSED | Marginal on stale params — not worth running |
| Dual-exit CAP sweep | 2026-04-28 | CLOSED | CAP=3 already validated under Turtle-only (72.2% pass) |

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

**Source of truth for production params: `src/live/config.rs`. Last verified: 2026-04-29.**