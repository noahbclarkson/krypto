# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-03 08:05 UTC. Critique cycle complete. ATR_RANK extensive sweep done (T∈[0..100]). VL=96 validated in live_compatible_wf (54/63 pass, Sharpe 7.652) but CONFLICTS with config.rs VL=8 — UNRECONCILED. T49 LOB NOBI, T52 BTC-ETH cointegration, ETF flow all overdue 3-4 weeks.*

---

## Critical Alert: Config Drift — VOL_LOOKBACK

**live_compatible_wf.rs:32 → VOL_LOOKBACK=96**
**src/live/config.rs:47 → VOL_LOOKBACK=8**

12x difference in smoothing window. The walk-forward validates VL=96 but the deployed bot uses VL=8. T50 must reconcile this before citing any live_compatible_wf results as truth.

---

## Critical Alert: ATR_RANK=24 Same-Harness Artifact Risk

ATR_RANK=24 was found on `live_compatible_wf.rs` and validated on the same harness (T∈[0..100]). EP=24 failed held-out after being found on the same harness. ATR_RANK=24 has NOT been held-out validated. The 54/63 pass rate reflects in-sample OOS optimization on a specific grid — it should be treated as a candidate, not a production default. T51 runs held-out validation.

---

## New Tasks (T49-T52)

- **T49: LOB NOBI Signal Harness** — Data already collected (daemon since 1862b57). One harness file. Compute daily depth imbalance → SG smoothing → z-score → test directional continuation. Lowest lift, highest value test in project history.

- **T50: VOL_LOOKBACK Reconciliation** — Run VL=8 vs VL=96 on Base5 × 7 windows. Commit reconciled value to BOTH live_compatible_wf.rs AND config.rs. No drift allowed.

- **T51: ATR_RANK=24 Held-Out Validation** — Run on pre-2021 data. If wins → promote. If loses → revert to T=5.

- **T52: BTC-ETH Cointegration** — First credible mean-reversion candidate. Rolling Johansen + spread z-score. 9 universes × 6 windows. If null → GRAVEYARD cleanly.

---

## Critical Alert: Sequential Optimization Pattern (EP=24 Repeating)

**AP=64 is the same trap as EP=24.** REGIME_ATR_PERIOD=64 was found on the same `live_compatible_wf.rs` harness that produced:
1. ATR_RANK_THRESHOLD=24 (found first on this harness)
2. REGIME_LOOKBACK=42 (found second on this harness)
3. REGIME_ATR_PERIOD=64 (found third on this harness)

Three sequential optimizations on the same OOS grid. EP=24 failed held-out validation. AP=64 must complete held-out validation before being trusted as production default.

**Anti-spin rule:** When 3+ params are optimized on the same harness in sequence, the latest param needs held-out validation before trust.

---

## Critical Alert: Same-Harness Artifact Pattern (VL=96)

**VOL_LOOKBACK=96 (UNCONFIRMED):** Found on the same 100-value sweep harness (VL=1..100 step 1) that produced VL=8 two days prior. Same-harness artifact pattern (EP=24). One day later, VL=96 was "found" using identical methodology.

**Rule:** Never re-run confirmed params at higher resolution on the same harness. VL=8 was settled. VL=96 is a same-harness artifact risk. **Do not promote VL=96 to production without T37 Base5-only held-out confirmation.**

---

## Critical Alert: Documentation Spiral, Not Discovery Spiral

The research loop is NOT closed — it's spinning. Last 8 commits: 5 docs/audits, 3 hyperopts (2 confirming already-known params). REGIME_LOOKBACK=42 sweep (196 values) confirmed the obvious. REGIME_ATR_PERIOD=64 is sequential optimization on the same harness that found T=24 and LB=42.

**Genuinely unresolved research topics:**
- VL=96 Base5 confirmation (T37)
- AP=64 held-out validation
- T38 complete equity export
- Fee model reality check (backtest uses 10bps, live likely ~4-5bps due to 70% maker fills)

**Genuinely novel untested idea:** Regime-Adaptive Exit (RAE) — conditional Chandelier multiplier (mechanistically different from the uniform multiplier failure).

---

## Top 3 Most Promising Unbuilt Ideas

### #1: T37 — VL=96 vs VL=8 Base5 Confirmation (MANDATORY)
**Status:** UNBUILT, 2+ sessions overdue.
**Why:** VL=96 found on same-harness artifact pattern. Must confirm on Base5 or remove claim.
**What:** Run `live_compatible_wf.rs` on Base5 only (6 windows) with VL=96 vs VL=8.
**If VL=96 wins:** Promote to config.rs.
**If VL=96 loses:** Remove claim from all files, keep VL=8.

### #2: AP=64 Held-Out Validation
**Status:** UNCONFIRMED — same-harness artifact risk.
**Why:** Three sequential optimizations on same OOS harness (T=24 → LB=42 → AP=64). EP=24 pattern.
**What:** Run AP=12 vs AP=64 on pre-2021 held-out data.
**If AP=64 wins held-out:** Confirm as production default.
**If AP=64 loses:** Revert to AP=12 (well-validated from prior sweep).

### #3: T38-COMPLETE — Full-History Equity Export from live_compatible_wf.rs
**Status:** PARTIAL, 3+ sessions.
**Why:** Live bot equity (75.1x) comes from progress_equity_curves.rs, not from live_compatible_wf.rs. Need clean equity export from actual live bot path.
**What:** Add equity CSV export to live_compatible_wf.rs, run full timeline, produce clean snapshot.

---

## Unbuilt Ideas (Priority Within Category)

### Infrastructure / Trust (Must Fix)
- [x] **T36**: Sync progress_equity_curves.rs to config.rs ✅ (CHAND_P=7, VL=8, ATR_RANK=24)
- [ ] **T37**: VL=96 vs VL=8 Base5 confirmation — MANDATORY, 2+ sessions overdue
- [ ] **AP=64 held-out**: Sequential optimization risk — validate or revert
- [ ] **T38-complete**: Full-history equity from live_compatible_wf.rs — 3+ sessions partial
- [ ] **S6 GRAVEYARD**: Move close_losers from "candidate" to GRAVEYARD (0 trades on Turtle-only)

### Research (Genuinely Untested)
- [ ] **T40**: Regime-Adaptive Exit (RAE) — vol-conditional Chandelier multiplier (conditional vs uniform — genuinely different mechanism)

### Blocked on Credentials
- [ ] **T9**: Live testnet (BLOCKED 5+ weeks on Noah's Binance testnet API keys)

---

## Tested and Rejected (Do Not Revisit Without New Mechanism)

| Strategy | Result | Key Reason |
|----------|--------|------------|
| ATR_ENTRY_MULT=0.94 | REJECTED | Held-out 10/18 vs baseline 11/18 |
| VOL_LOOKBACK=96 | UNCONFIRMED | Same-harness artifact (EP=24 pattern) — keep VL=8 until T37 confirms |
| ATR_EMA [1..200] | CONFIRMED NULL | No improvement anywhere in range |
| FRESHNESS_COOLDOWN [0..70] | CONFIRMED NULL | cd=0 optimal |
| HOLD_MAX [1..100] | CONFIRMED NULL | HM=12 optimal, no benefit from longer |
| CHAND_MULT [1.5..5.0] | CONFIRMED NULL | M=2.30 optimal |
| ATR_ENTRY_MULT [0.00..2.00] step 0.01 | CONFIRMED NULL | EM=0.00 wins |
| **S6 close_losers I=5** | **GRAVEYARD** | **Incompatible with Turtle-only live exit (0 trades). Chandelier dual-exit ONLY.** |
| Donchian sleeve | REJECTED | 63% < 69.1% guardrail |
| ATR-norm position sizing | REJECTED | Equal capital optimal; ATR-norm inverts vol ranking |
| Asymmetric exit | REJECTED | All configs identical |
| Mid-caps | REJECTED | 60% < 70% threshold |
| 4h Multi-Timeframe Turtle | GRAVEYARD | 1/20 pass — structural timeframe incompatibility |
| BollingerReversion | GRAVEYARD | 0/288 OOS — signal actively harmful |
| Position scaling overlays | GRAVEYARD | All failed — Chandelier already manages it |
| Vol-contingent Chandelier (uniform) | GRAVEYARD | All configs identical — mechanism doesn't work |
| ATR entry × volume confirmation | REJECTED | 40 configs, all inferior to no filter |
| BTC correlation entry filter | REJECTED | All variants lose to baseline on every metric |
| A/D Dual-Hat static sleeve | REJECTED | Below-random win rate, -6.2% vs Turtle |
| CTREND fixed 25% sleeve | REJECTED | Sharpe destroyed 1.38→0.33 |
| ATR-norm position sizing | REJECTED | Inverts dollar-volume ranking |
| Rebalancing trim_losers | REJECTED | Identical Sharpe, lower return |
| REGIME_ATR_PERIOD=64 | UNCONFIRMED | Sequential optimization on same harness (EP=24 pattern) |

---

## Anti-Overfitting Rules (Updated 2026-05-02)

1. Minimum 3-window (5.5%) improvement on OOS before accepting any param change
2. No sequential optimization on same data (EP=24 lesson — applies to AP=64)
3. Never re-run confirmed params at higher resolution on the same harness (VL=96 lesson)
4. Held-out validation required for marginal wins (< 3 windows over baseline)
5. Equity curve dominance required (>80% of time bars)
6. **Absolute guardrails over relative improvement** (Donchian: +11% Sharpe but 63% pass < 69.1% guardrail → REJECTED)
7. **Sequential optimization detection:** When 3+ params optimized on same harness in sequence, latest param needs held-out validation before trust

---

## Key Insight: All Metrics Are Upper Bounds

Everything in HALL_OF_FAME.md is a simulation maximum. The real validation path is live testnet paper trading + comparing actual vs predicted metrics.

**Biggest unmeasured risk:** Optimizing into a bull market. No 12-18 month sustained bear window in OOS data. 2026 YTD (-22.7%) is the closest proxy but only 4 months. A genuine prolonged bear market is the real test.

**Fee model reality check:** Backtest uses 10bps taker, but microstructure analysis shows ~70% maker fills on entries → real expected cost ~4-5bps. Backtest may be pessimistic by 2-3x. Live could outperform walk-forward numbers.

**Source of truth for production params: `src/live/config.rs`. Last verified: 2026-05-01.**

---
### Bypass Blocker: Local Mock Exchange
**Status:** UNBUILT.
**Why:** Noah's API keys have been blocking testnet for 5+ weeks. We need to test the `bot.rs` execution logic (order placement, state machine, latency handling).
**What:** Build a lightweight local Rust HTTP/WS server that mocks the `binance-rs-async` endpoints we use. Seed it with historical 1m data to simulate fills and slippage locally.
