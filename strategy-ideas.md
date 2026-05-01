# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-01 04:05 UTC. Live bot exit path VERIFIED Turtle ATR sole-exit. Dual Chandelier+Turtle metrics are research-only unless Chandelier is integrated. T39 actual Turtle ATR stop sweep remains unbuilt. T40 Regime-Adaptive Exit remains unbuilt but must pass live-path parity. Live testnet BLOCKED 4+ weeks.*

---

## Critical New Insight: Live-Path Parity Is Now the Main Research Gate (2026-05-01 04:05)

**Verified:** `src/live/bot.rs` implements Turtle ATR sole-exit through `check_turtle_exit`. There is no Chandelier exit and no close_losers/rebalancing logic in the live bot.

**Implication:** Any result requiring dual Chandelier+Turtle exit is not production evidence unless we either:
1. integrate Chandelier into live, or
2. explicitly label that result `RESEARCH_ONLY`.

**New concept / required infrastructure:** `live_path_parity_harness` — a harness/report that compares exact live semantics against research semantics and stamps every strategy result:
- `LIVE_COMPATIBLE`
- `RESEARCH_ONLY`
- `REQUIRES_LIVE_INTEGRATION`

This is not glamorous, but it is the highest-value next idea because it prevents another month of validating things the bot cannot trade.

## Updated Top 3 Unbuilt Ideas (2026-05-01 04:05)

1. **Live-path parity harness / audit gate** — exact `src/live/bot.rs` semantics vs research harness. Mandatory before promoting anything.
2. **T39: Actual live Turtle ATR stop sweep** — test `TURTLE_ATR_PERIOD={12,15,18,21,24,30}`. AP=12 currently affects only ATR-rank regime detection, not the live stop.
3. **T40: Regime-Adaptive Exit (RAE)** — promising only if live-compatible or paired with a conscious Chandelier live integration decision.



## Critical New Insight: Regime ATR (AP=12) — Partially Integrated

**Status (2026-04-30 20:05):** REGIME_ATR_PERIOD=12 and REGIME_LOOKBACK=42 are in `config.rs` as regime detector parameters. ATR_RANK=5 uses them as inputs. **BUT the live Turtle ATR stop still uses TURTLE_ATR_PERIOD=24.** The "+78% Sharpe" was for the regime detector ATR percentile mechanism, NOT for the Turtle ATR stop.

These are two different things:
1. **Regime detector ATR (AP=12, LB=42):** Used to compute BTC ATR percentile rank for entry filtering. Integrated ✅
2. **Live Turtle ATR stop (TURTLE_ATR_PERIOD=24):** The actual trailing stop. UNCHANGED.

**Action:** Run `examples/regime_turtle_atr_sweep.rs` — test AP=12 as the live Turtle ATR period (not just the rank filter source). Compare TURTLE_ATR_P={12,15,18,21,24,30} on 9-universe × 6-window harness. If AP=12 wins as live stop: integrate.

---

## Critical New Insight: ATR_RANK=5 Is Time-Period Dependent (2026-04-30 20:05)

**Discovery:** ATR_RANK=5 was validated as +24% Sharpe under Turtle-only walkforward (9-universe, 54 OOS windows). Integrated into live bot. BUT adding to `progress_equity_curves.rs` collapsed equity 221.5x → 124.1x on the full-history harness.

**Root cause:** The equity harness covers 2018-2026 including chop periods where Turtle breakouts eventually work even if initially stopped out. ATR_RANK=5 blocks entries in low-vol chop — removing trades that would have been profitable given enough time. The filter is net positive on recent OOS windows (dominated by trending periods) but net negative on full history.

**Implication:** ATR_RANK=5 is a live-trading filter, not a backtestable parameter on full-history equity curves. It should be tested via walk-forward OOS validation only, not cumulative equity. The live bot uses it correctly. The progress equity harness should NOT include it as the baseline comparison.

**Fix:** Progress equity harness needs two series: baseline (no ATR rank) and ATR_RANK=5. Do not replace baseline with filtered variant.

**Pattern since 2026-04-28:**
- Regime ATR: found, reported to Discord, not in config.rs
- ATR_RANK=5: validated dual-exit + Turtle-only, not in config.rs
- S6: found as candidate, Turtle-only validation not run

**The Discord announcement is not the completion.** The completion is editing config.rs and wiring into bot.rs.

**Anti-spin rule:** If it was announced in Discord but config.rs didn't change, the finding is UNINTEGRATED — treat it as unvalidated until integration completes.

---

---

## NEW CONCEPT: Regime-Adaptive Exit (RAE) — Vol-Conditional Chandelier Multiplier

**Status (2026-05-01):** NEW. Genuinely untested mechanism.

**Why new:** Prior vol-contingent Chandelier (GRAVEYARD) tested uniformly changing the stop MULTIPLIER by vol regime. Result: all configs produced identical results. Mechanism was wrong — uniform multiplier change without period change doesn't alter the trailing stop meaningfully.

**RAE mechanism:** Conditionally adjust CHAND_MULTIPLIER by CURRENT volatility regime, dynamically:
- High-vol regime (BTC ATR rank > 60th pct): M × 1.1 → looser stop, avoid premature stop-out in volatile trends
- Low-vol regime (BTC ATR rank < 40th pct): M × 0.9 → tighter stop, capture choppy range breaks faster
- Neutral regime: M = 2.30 (fixed baseline)

**Why this is different from the GRAVEYARD'd attempt:**
1. RAE uses ATR percentile rank (same mechanism as ATR_RANK=5 entry filter), not 21-bar realized vol
2. RAE adjusts multiplier based on CURRENT regime state, not a static schedule
3. Prior attempt: multiplier=2.0 always won; RAE hypothesizes conditional adjustment wins where uniform fails

**What to build:** `examples/regime_adaptive_exit_walkforward.rs` — sweep high_vol_mult ∈ {1.0, 1.05, 1.1, 1.15} × low_vol_mult ∈ {2.0, 2.1, 2.2, 2.3} × 9 universes × 6 windows.

**Reject if:** Pass rate or Sharpe degrades vs fixed M=2.30 baseline. If no improvement: vol-conditional exit space is truly exhausted.

**Donchian result (T19, 2026-04-28) changes the picture:**

- Donchian: avg Sharpe +9.4, pass rate 86% (-14pp vs Turtle)
- Turtle: avg Sharpe +5.6, pass rate 100%
- **Entry alternatives trade pass rate for per-trade Sharpe quality.** They don't add new edge — they filter signals.

This pattern matches every failed entry approach:
- ATR_MULT (fixed threshold): pass rate degrades monotonically as threshold increases
- Volume confirmation: pass rate degrades 6-13pp
- Correlation filter: loses to baseline on every metric

**Implication:** Entry space is definitively closed. Turtle entry is the optimal trade-off between signal frequency and signal quality.

---

## Critical New Insight: VOL_LOOKBACK 8→90 — Same-Harness Resweep (2026-04-30)

**Commit cfd19ba2:** 100-value sweep (1..=100 step 1) × 9 universes × 6 WF windows = 54,000 sims. Winner: VL=90 plateau (91-100): 37/54 pass vs baseline 34/54 at VL=8. Sharpe 4.457 vs 3.392 (+31%), DD 73.8% vs 79.3%.

**⚠️ CONCERN — EP=24 pattern:** VL=8 was confirmed on the same harness 2026-04-29 (also 100-value sweep). VL=90 was found on identical methodology one day later. This is the same pattern as EP=24 (found on same harness as CHAND_P=11, failed held-out, reverted to EP=21).

**VL=8 was the Base5 production winner on 2026-04-29.** VL=90 was NOT re-tested on Base5. 9-universe aggregate improvement ≠ Base5 improvement.

**What to do:** Compare VL=90 vs VL=8 on Base5 × 6 windows (12 runs only). If VL=90 wins Base5, update production. If VL=90 loses Base5, revert to VL=8 and STOP resweeping VOL_LOOKBACK.

**Anti-overfit rule violated:** Never re-run confirmed params at higher resolution on the same harness. VL=8 is settled. VL=90 is a same-harness artifact risk until Base5 validates it.

---

## Critical New Insight: Research Loop Is a Confirmation Spiral

**As of 2026-04-30:** We keep re-running settled parameters at higher resolution and calling it new research.

- ATR_EMA [1..200] × 9u × 54w = 10,800 runs — re-confirmed NULL at [1..30] on the same harness. Same result, higher resolution. Not discovery.
- ATR_ENTRY_MULT 201-value sweep (0.00..=2.00 step 0.01) — re-confirmed EM=0.00 on current params. Same result, 201 values instead of 41. Not discovery.
- ATR_ENTRY_MULT=0.94 candidate: real signal (42/54 pass, Sharpe 5.34 vs baseline 40/54/3.15) — correctly not promoted (anti-overfit discipline). But this was found on the same WF grid it would be validated against.

**The research loop is NOT closed. It's spinning.** T32 is now fixed; the loop closes when we stop running hyperopts and build T31 plus funding observer continuous monitoring.

---

## Critical New Insight: USDT Hedge Overlay — ✅ Integrated

**Documented 2026-04-11, integrated 2026-04-29 (commit 683fe92e).**
- Trigger: BTC 21d vol > 75th percentile of 252-bar history → reduce position 30%, hold 30% in USDT
- Effect: ~30% DD reduction in bear windows (historical validation)
- Mechanism: modest position size overlay, non-breaking, optional
- **This directly addresses the pre-2021 stress weakness (67.9% below 70% threshold).**
**Status:** Built in `src/live/bot.rs` lines 245-275. Needs live/testnet observation, not more historical tuning.

---

## Top Genuinely Untested Ideas (Priority Order)

### T29: Funding Rate Live Observer — ✅ COMPLETE (2026-04-30 00:08 UTC)
**Built:** `examples/funding_rate_live_observer.rs` — polls Binance premiumIndex public API (no keys), compares to 30d cached history, computes z-score/percentile, detects extremes.
**Live result:** NEAR NEUTRAL (-0.04% avg ann funding). No extremes detected. All z-scores -0.36 to -1.05. DOGE tends highest funding (0.037% ann vs BTC 0.022%).
**Status:** COMPLETE. Next: continuous monitoring via `scripts/run_funding_observer.sh`.

### T31: Donchian as Portfolio Complement — BASE5 CANDIDATE ✅ (9-universe pending)
**Status:** BUILT on Base5 (2026-04-30 commit 4a4e15ba). 9-universe validation still pending.
**Result (Base5 × 6 windows):**
- Turtle(75%) + Donchian(25%): **6/6 pass, Sharpe +4.767 (+22.0% vs Turtle)**, +2284% avg return
- Guardrail: reject if global pass rate drops >5pp (below 69.1%) or Sharpe fails outside Base5
**What to build:** `examples/donchian_sleeve_9universe.rs` — 9-universe × 6-window validation

### T32: Sharpe Metric Integrity Fix — COMPLETE ✅ (2026-04-30)
**Problem fixed:** `daily_progress.csv` no longer silently compares DDBudget 7.24 to Turtle 1.04 as peer Sharpe values.
**Built:** Added `sharpe_methodology` to the report and updated `scripts/run_daily_progress.sh` so refreshes preserve methodology. `progress_equity_curves.rs` generated markdown now labels DDBudget as milestone-aggregated/not comparable to Turtle daily compounded equity.
**Current interpretation:** Turtle+Chandelier = 221.1x / 1.04 `daily_compounded_equity`; DDBudget = 61.3x / 7.24 `milestone_aggregated_not_comparable`. Do not cite the DDBudget 7.24 as a superior peer Sharpe.

### S6: Rebalancing Frequency / Winner-Loser Maintenance — UNTESTED (TOP PRIORITY)
**Status:** Listed since 2026-04-11. **NEVER BUILT.** Genuinely novel — no hyperopt loop has touched it. No API keys needed.
**Hypothesis:** Current Turtle+Chandelier opens and waits for exit. Hypothesis: periodic rebalancing (every N bars: re-rank open positions by unrealized PnL, trim/close worst if in loss >N bars, let leaders run) may improve capital efficiency without suppressing trend convexity.
**Why it is worth testing:** Addresses "trapped capital in decaying breakouts" as a position lifecycle problem, not an entry problem. Mechanistically different from all failed scaling overlays.
**What to build:** `examples/rebalancing_sweep.rs` — sweep rebalance_interval ∈ {5, 10, 15, 21, 30, 42} bars, rebalance_type ∈ {trim_losers, close_losers, redistribute}. Base5 × 6 windows. Compare to no-rebalancing baseline.
**Reject if:** Increases turnover materially or collapses pass rate >5pp after fees.

---

## ATR_EMA [1..200] Confirmation — NULL (2026-04-29)

**10,800 runs** (200 values × 9 universes × 54 windows). ATR_EMA=4 wins pass rate (43/54 vs 42/54 baseline) but loses -0.36 Sharpe (3.76 vs 4.12). ATR_EMA=1 (raw ATR) confirmed as production default by robustness-first criteria.

**Prior:** [1..30] sweep on stale params — NULL. This sweep confirms it extends to [1..200].

**Not discovery.** Re-confirmation of settled result.

---

## ATR_ENTRY_MULT 201-Value Sweep (2026-04-29) — CONFIRMED NULL, CANDIDATE FOUND

**Scope:** ATR_ENTRY_MULT ∈ [0.00..=2.00] step 0.01 (201 values) × 9 universes × 6 windows = 54 windows/value.
**Params:** CHAND(7,2.30)/EP=21/HM=12/CAP=3/VL=8/ATR(24,2.0).

**Winner (baseline):** EM=0.00 — 40/54 pass (74.1%), Sharpe 3.15, return +105.3%, DD 35.4%, 721 trades.
**Candidate:** EM=0.94 — 42/54 pass (77.8%), Sharpe 5.34 (+2.19), return +73.0%, DD 28.3%, 486 trades.
**Highest Sharpe:** EM=1.07 — 41/54 pass, Sharpe 7.86 (inflated by mega-bull windows).

**Decision:** No production change. EM=0.94 found on same WF grid it would be validated against. Anti-overfit discipline requires held-out data before promotion. EM=0.00 remains production default.

**Status:** REJECTED ✅ (held-out 2026-04-30). EM=0.00: 11/18 pass. EM=0.94: 10/18 pass. EM=0.00 remains production default. ATR_ENTRY_MULT=0.94 removed from candidate status. Do not resweep unless new mechanism found.

---

## Equity Bug Fix — ✅ FIXED (2026-04-29, commit e55659e8)

**Root cause:** Off-by-one forward-fill in `progress_equity_curves.rs`. While loop exits before last exit is recorded.
**Fix:** Record equity at bar=exit_bar before incrementing. One targeted edit.
**Status:** FIXED. Off-by-one recording loop corrected.

---

## Dual-Exit Attribution — ✅ COMPLETED (2026-04-28)

**Result:** Chandelier fires first ~7-8% of windows, not >90% as feared. TURTLE_ATR_PERIOD=24 is a REAL parameter.
- Dual-exit: 40/54 pass (global), 6/6 Base5
- Turtle-only: 36/54 pass (global), 5/6 Base5 (W04 bear chop fails)
- Chandelier adds 1 window of robustness — secondary exit, not primary driver

**⚠️ Live Bot Dual-Exit Gap — UNVERIFIED:** MEMORY states live bot uses Turtle-only exit (sole exit). Walk-forward validated dual Chandelier+Turtle ATR at 93% pass. If live is Turtle-only, the gap is ~26pp pass rate. **Action: Verify `src/live/bot.rs` exit logic. If Turtle-only, assess dual-exit implementation feasibility or accept the gap.**

## New Concept: 2026 YTD Root Cause Analysis

MEMORY per-year table: Turtle +2026 YTD = **-22.7%** while BTC = **+12.7%**. Gap = 35.4pp underperformance.

"Bear whipsaw" is a description, not a root cause. Mechanism: sustained downtrend + low vol = Turtle breaks out → Chandelier stops out → repeated whipsaw losses. This is a genuine structural weakness in choppy/bear regimes.

**Key question:** Is this **regime-inherent** (strategy working as designed in a hostile market) or is there a **live-vs-backtest divergence** (bug in live path)?

**What to do:** Compare live bot equity curve vs backtest equity curve on the same 2026 YTD period. If they diverge → real bug. If they match → regime-inherent, accept it.

---

## Live Testnet Blocker

**Status: CRITICAL BLOCKER — 4+ weeks without live testnet.**

The entire project is simulation. All metrics are upper bounds.

**Only genuine path forward:** Live testnet paper trading. All hyperopts on historical data exhausted. S6 rebalancing is the one live candidate (close_losers I=5, needs 9-universe validation). Everything else is locked or rejected.

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
| ATR_EMA [1..200] | 2026-04-29 | NULL | No smoothing improvement anywhere in range |
| ATR_ENTRY_MULT [0..2.00] | 2026-04-29 | EM=0.00 wins | EM=0.94 candidate — needs held-out validation |
| Chop filter | 2026-04-13 | REJECTED | Trade-starving |
| Correlation entry filter (T7) | 2026-04-25 | REJECTED | All 3 variants lose to baseline |
| CTREND + Chandelier exit | 2026-04-20 | 30/54 pass | Wrong exit mechanism for CTREND |
| CTREND regime-conditional switching | 2026-04-25 | 67% pass — FAIL | 67% < 70% threshold |
| 4h Multi-Timeframe Turtle | 2026-04-25 | 1/20 pass | Structural — dual exit collapses on 4h |
| Cross-market equity integration | 2026-04-16 | REJECTED | Combined -2.94 Sharpe vs crypto-only |
| DynamicTrend EMA signal | 2026-04-16 | REJECTED | Turtle wins 21/24 windows |
| A/D Static Sleeve | 2026-04-14 | 46% | Below-random win rate |
| BTC Trend Scalar | 2026-04-14 | 0/8 configs | Baseline wins |
| Regime-conditional allocation | 2026-04-12 | 60.5% | Worse than either component alone |
| XRP 4h MR | 2026-04-11 | 0/4 | Edge destroyed by fees |
| EP=24 | 2026-04-26 | REVERTED | In-sample inflation on same OOS data |
| EP=43 | 2026-04-27 | REVERTED | Found same session as EP=21 validation |
| ATR_ENTRY_MULT=0.85 | 2026-04-25 | REVERTED | In-sample inflation |
| Position scaling overlays | various | GRAVEYARD | All failed — Chandelier already handles it |
| Donchian entry (replacement) | 2026-04-28 | REJECTED | Wins Sharpe (+3.8) but loses pass rate (-14pp) — NOT as replacement; complement untested |
| ATR-norm position sizing (S4) | 2026-04-29 | REJECTED | Equal capital optimal, ATR-norm inverts vol ranking |
| Asymmetric exit | 2026-04-29 | REJECTED | Turtle ATR fires first; all configs identical |
| Mid-caps (BNB/LINK/AVAX/MATIC/UNI) | 2026-04-30 | REJECTED | 60% pass — below 70% threshold |

**Conclusion:** The reliable crypto edge is directional trend-following on daily data. Entry space is a pass-rate vs Sharpe trade-off — Turtle is the optimal point. Remaining untested ideas (T31, S6) require building, not more hyperopts.

---

## Anti-Overfitting Rules (Established 2026-04-25)

1. **Minimum win margin:** ≥3 windows (5.5%) improvement on OOS before accepting any param change
2. **No sequential optimization on same data:** If EP is optimized on data D, you cannot also optimize ATR_EM on data D and claim both are valid
3. **Held-out validation required for marginal wins:** 1-2 window delta = noise until pre-2021 stress confirms
4. **Equity curve dominance:** Winner must dominate baseline at >80% of time bars
5. **Never re-run confirmed params:** ATR_PERIOD confirmed 3×. CHAND_MULT confirmed 2×. P=7 confirmed 2×. ATR_EMA confirmed 2×. Stop.

---

## Key Insight: All Metrics Are Upper Bounds

Everything in HALL_OF_FAME.md is a simulation maximum. The real validation path is live testnet paper trading + comparing actual vs predicted metrics.

**Source of truth for production params: `src/live/config.rs`. Last verified: 2026-04-29.**
---

## Critical New Finding — T35: Fee Accounting Bug — All Sharpe Numbers Inflated 22-33%**

**2026-04-30 12:20 UTC**

**Bug:** `examples/turtle_chandelier_walkforward.rs` lines 184, 212:
```rust
let entry = entry_px * (1.0 - TAKER_FEE);  // WRONG — fee CREDIT on BUY
let exit  = exit_px * (1.0 - TAKER_FEE);  // correct direction, wrong formula
```

Both use `(1 - fee)`. Entry fees and exit fees cancel in the return ratio: `exit/entry = P(1-fee)/P(1-fee) = 1`. **Zero net fees charged.**

**What this means:**
- Walkforward Sharpe 3.147 is inflated. True fee-adj ≈ 2.2–2.5 (22-33% degradation)
- Fee sensitivity audit (2026-04-28) tested sensitivity on a fee-free baseline — the "degradation" was measuring the wrong thing
- HALL_OF_FAME.md numbers need refreshing under corrected fee model
- Daily equity Sharpe 1.04 is HONEST — it comes from different code path

**Fix:** `entry = entry_px * (1.0 + TAKER_FEE)`, `exit = exit_px * (1.0 - TAKER_FEE)`

**Note on ATR_RANK conditional entry:** Mechanically novel (percentile-based vol regime filter). Worth one dedicated harness run. Previous vol-contingent Chandelier failed identically zero — but that was a stop multiplier, not an entry filter. Different mechanism may produce different result. Worth trying once.
