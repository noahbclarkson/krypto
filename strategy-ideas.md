# Strategy Ideas — Krypto Research Log

*Last updated: 2026-04-29 13:24 UTC. Critique cycle. Equity bug FIXED (e55659e8). USDT hedge INTEGRATED (683fe92e). Daily reporting STALE. Research loop CLOSED. Live testnet BLOCKED 3+ weeks.*

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

## Critical New Insight: USDT Hedge Overlay — ✅ Integrated

**Documented 2026-04-11, integrated 2026-04-29 (commit 683fe92e).**
- Trigger: BTC 21d vol > 75th percentile of 252-bar history → reduce position 30%, hold 30% in USDT
- Effect: ~30% DD reduction in bear windows (historical validation)
- Mechanism: modest position size overlay, non-breaking, optional
- This directly addresses the pre-2021 stress weakness (67.9%) without re-optimizing entry/exit params

**Status:** Built in `src/live/bot.rs` lines 245-275. Needs live/testnet observation, not more historical tuning.

---

## Top Genuinely Untested Ideas (Priority Order)

*(Updated 2026-04-29 13:24 — historical sweep loop CLOSED; next ideas must target reporting integrity, live observability, or truly different exit/risk mechanics)*

### S4: ATR-Normalized Position Sizing — ✅ REJECTED (2026-04-29 03:10 UTC)
**Result:** REJECTED. Equal capital allocation is optimal.
- Equal capital: 6/7 pass (86%), Sharpe 10.2
- ATR norm 10K: 4/7 pass (57%), Sharpe 53.5 (inflated by W3 mega-bull)
- ATR norm 20K: 4/7 pass (57%), Sharpe 107.0
- Root cause: ATR normalization INVERTS dollar-volume ranking. Low-vol assets get disproportionately large positions.
- W4 catastrophic failure: equal_capital +152% (39% DD) vs ATR_norm -1866% (1840% DD)
**Files:** `examples/s4_atr_norm_position_sizing.rs`, `snapshots/s4_atr_norm_position_sizing.md`
**Status:** CLOSED. Research loop TRULY CLOSED.

### USDT Hedge Overlay — ✅ INTEGRATED (2026-04-29, commit 683fe92e)
**Concept:** Vol-regime position sizing overlay. Reduce position 30% when BTC 21d vol > 75th pct of 252-bar history.
- Documented 2026-04-11: ~30% DD reduction in bear windows
- Mechanism: `vol_pct = rank_21d_atr(bar) / 252`. If vol_pct > 0.75 → hedge_ratio=0.30 (30% notional in USDT, 70% in position).
- Risk: modest, non-breaking, optional overlay — only activates in high-vol regimes
- **This directly addresses the pre-2021 stress weakness (67.9% below 70% threshold).**
- Action: Observe in live/testnet; no further historical re-sweep.
**Status:** ✅ INTEGRATED in src/live/bot.rs lines 245-275. Vol-regime position sizing active.

### T22: Dual-Exit Attribution — ✅ COMPLETED (2026-04-28 20:00 UTC)
**Result:** Chandelier fires first ~7-8% of windows, not >90% as feared. TURTLE_ATR_PERIOD=24 is a REAL parameter.
- Dual-exit: 40/54 pass (global), 6/6 Base5
- Turtle-only: 36/54 pass (global), 5/6 Base5 (W04 bear chop fails)
- Chandelier adds 1 window of robustness — secondary exit, not primary driver
**Conclusion:** Prior dual-exit hyperopts (ATR_P=24, ATR_M=2.0) are valid — they fire first in ~90% of trades.
**Status:** CLOSED. TURTLE_ATR_PERIOD=24 hyperopt confirmed as genuine, not noise.

### T24: Equity Bug Fix — ✅ FIXED (2026-04-29, commit e55659e8)
**Root cause:** Off-by-one forward-fill in `progress_equity_curves.rs`. While loop exits before last exit is recorded. Forward-fill then sets `equity_curve[total]` (row 2086) to 1.0 instead of the actual final equity.
**Evidence:** Row 2086 = 1.0, row 2087 = 221.5x. Two rows of output — the harness completes successfully, so the symptom was masked by the parquet refresh.
**False claim:** Commit `9fb2a81c` ("equity bug fixed") modified ZERO Rust source lines. No code was changed. This is a pattern of reporting desired state vs actual state.
**Fix:** One targeted edit to the equity recording loop in `examples/progress_equity_curves.rs`. Record equity at bar=exit_bar before incrementing.
**Status:** FIXED. Off-by-one recording loop corrected.

### T20: ATR-Rank Conditional Filter — ASSESSED (not worth running)
**Existing results** from stale harness (CHAND_P=15/M=1.50, EP=21, HM=45): baseline 54% pass, t=20/30 shows +2pp pass at best. Not decisive.
**Why not running properly:** Donchian already showed entry filter space trades pass rate for Sharpe. T20 would likely show same pattern. Live testnet is the only real validator.
**Status:** CLOSED — not worth the compute. Entry space definitively exhausted.

### T27: Asymmetric Exit Architecture — NEW / PROMISING
**Hypothesis:** Trend-following payoff is convex; exits should be asymmetric. Cut losers faster with a tight hard stop (e.g., ATR×0.5 from entry), while letting winners use a looser trailing Chandelier (e.g., ATR×3.0) plus Turtle ATR as secondary fail-safe.
**Why it is different:** Not another entry filter. It changes loss truncation and winner convexity, the core economics of Turtle-style systems.
**Test:** Baseline vs asymmetric soft-only vs asymmetric hard+soft across Base5×7 windows first. Accept only if pass rate does not degrade and improvement is not a one-window artifact.

### S5: Funding-Rate Regime Overlay — NEW / LIVE-DATA CANDIDATE
**Hypothesis:** Extreme funding regimes identify crowded positioning. When aggregate perp funding is deeply negative or violently flipping, reduce spot-long exposure or delay new entries.
**Why it matters:** Funding is market microstructure data absent from current daily OHLCV harnesses. This could catch bear/chop stress that price-only filters miss.
**Caution:** Do NOT optimize thresholds on stale funding histories without enough samples. First build a live observer/dashboard, then decide if it deserves a rule.

### S6: Rebalancing Frequency / Winner-Loser Maintenance — NEW
**Hypothesis:** Current logic opens and waits for exit. Periodic maintenance (e.g., rebalance every 5 bars, trim losers, do not trim winners) may reduce capital trapped in decaying breakouts without suppressing trend convexity.
**Why it is worth testing:** Position lifecycle, not entry. Could improve capital efficiency without adding a new alpha signal.
**Reject if:** It increases turnover materially or collapses pass rate after fees.

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

**The USDT hedge overlay** (reduce position 30% when BTC 21d vol > 75th pct of 252d history) is now integrated. That does NOT solve the limitation; it only reduces exposure during high-vol stress.

**What we should do:** Acknowledge this as a known limitation in strategy scope — works best in trending bull markets, fragile in choppy/bear. Don't pretend we can eliminate it without live data. Observe the hedge in testnet/live conditions and avoid additional parameter tuning unless new data justifies it.

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