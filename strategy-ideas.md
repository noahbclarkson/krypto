# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-02 00:21 UTC. Research loop in DOCUMENTATION SPIRAL mode (3 sessions, no new strategy work). T40/T42/T43 genuinely untested. Live testnet BLOCKED 5+ weeks.*

---

## Critical Alert: Same-Harness Artifact Pattern (EP=24/VL)

**EP=24 (REVERTED):** Found on the same harness (EP sweep) during the same session that validated CHAND_P=11. Sequential optimization on same OOS data → in-sample inflation. Reverted after held-out confirmed EP=21 wins.

**VOL_LOOKBACK VL=96 (UNCONFIRMED):** Found on the same 100-value sweep harness (VL=1..100 step 1, 9 universes × 6 windows) that produced VL=8 on 2026-04-29. One day later, VL=96 was "found" using identical methodology. This is the same pattern as EP=24.

**Rule:** Never re-run confirmed params at higher resolution on the same harness. VL=8 was settled. VL=96 is a same-harness artifact risk. **Do not promote VL=96 to production without T37 Base5-only held-out confirmation.**

---

## Critical Alert: Infrastructure Is Broken, Not Research

The research loop is GENUINELY CLOSED. Every testable mechanism has been tested:
- Entry alternatives: ATR filter, volume confirmation, correlation filter, Donchian, EMA crossover, CTREND, A/D — all rejected
- Exit alternatives: vol-contingent Chandelier (uniform), asymmetric exit, ATR_EMA, chop filter — all rejected
- Position sizing: ATR-norm, trend scalar, USDT hedge, drawdown trigger — all marginal or rejected
- Portfolio: equity integration, A/D sleeve, Donchian sleeve, CTREND sleeve — all rejected

**The problem is not that we need more research. The problem is we can't trust the numbers we have because the harness that produces them doesn't match the live bot.**

---

## Critical Alert: All Turtle-Only Metrics Are Stale (2026-05-01)

**Bug fix (commit `79a3442d`):** `src/live/bot.rs` was using wrong ATR period for Turtle ATR buffer, wrong stop direction (lowest_low - ATR instead of highest_high - ATR), and not enforcing HOLD_MAX during ATR warmup. Fixed 2026-05-01 02:00 UTC.

**ALL prior Turtle-only walk-forward results are stale.** The following require re-run under corrected live-path semantics before they can be cited as production evidence:
- ATR_RANK=5 (Turtle-only validation pre-dates bug fix)
- TURTLE_ATR_PERIOD=24 (sweep results were degenerate because stop was unreachable)
- Any walk-forward that used "Turtle-only exit" as its exit mechanism

**Action required:** T38 (corrected live-path walk-forward) after T36 syncs progress harness to config.rs.

---

## Top 3 Most Promising Unbuilt Ideas

### #1: T38 — Corrected Live Turtle-Only Walk-Forward (CRITICAL — BLOCKED ON T36)
**Status:** PARTIAL/STALE. Most important unstarted task.
**Why:** All Turtle-only metrics are stale after the 2026-05-01 bug fix. Without this, nothing we say about Turtle-only performance is credible.
**What:** Rebuild harness matching `src/live/bot.rs` exactly — Turtle-only exit, correct ATR buffer, HOLD_MAX independent of warmup. Use CHAND_PERIOD=7, VL=8 from config.rs. Export full-history equity CSV. Label `LIVE_COMPATIBLE` vs `RESEARCH_ONLY`.
**Dependency:** T36 must complete first (sync progress harness to config.rs) — this is a prerequisite for a trustworthy equity number.

### #2: T37 — VL=96 vs VL=8 Base5-Only Confirmation
**Status:** UNBUILT. Small, definitive one-run test.
**Why new:** VL=96 is unconfirmed. Same-harness artifact pattern (EP=24). Either it wins on Base5 and should be promoted, or it doesn't and the claim should be removed.
**What:** Run `live_compatible_wf.rs` on Base5 only (6 windows) with VL=96 vs VL=8. If VL=96 wins on Base5: promote. If not: VL=96 claim is removed from all files.
**Value:** Eliminates noise from claims. Makes production default trustworthy.

### #3: Regime-Adaptive Exit (RAE) — Vol-Conditional Chandelier Multiplier
**Status:** UNBUILT. Genuinely novel mechanism.
**Why different from prior vol-contingent attempt:** Prior attempt (GRAVEYARD) tested UNIFORM multiplier change — all configs produced identical results. RAE proposes CONDITIONAL adjustment: high-vol → M×1.1 (looser, avoid premature stop-out), low-vol → M×0.9 (tighter, capture choppy range breaks faster), neutral → M=2.30.
**What to build:** `examples/regime_adaptive_exit_walkforward.rs` — grid of high_vol_mult × low_vol_mult × 9 universes × 6 windows.
**Reject if:** Pass rate or Sharpe degrades vs fixed M=2.30. This closes the vol-conditional exit space definitively.

---

## Unbuilt Ideas (Priority Within Category)

### Infrastructure / Trust (Must Fix)
- [x] **T36**: Sync progress_equity_curves.rs to config.rs — CRITICAL (CHAND_PERIOD=11 vs 7, VL mismatch)
- [ ] **T38**: Corrected Turtle-only walk-forward (BLOCKED on T36 — equity number unreliable until synced)
- [ ] **T37**: VL=96 vs VL=8 Base5-only confirmation (small, definitive)
- [ ] **S6 GRAVEYARD**: Move close_losers from "candidate" to GRAVEYARD (0 trades on Turtle-only exit)

### Research (Genuinely Untested)
- [ ] **T40**: Regime-Adaptive Exit (RAE) — vol-conditional Chandelier multiplier (build once if approved)

### Blocked on Credentials
- [ ] **T9**: Live testnet (BLOCKED 4+ weeks on Noah's Binance testnet API keys)

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

---

## Research Loop Status: CLOSED (Genuinely)

**CLOSED by exhaustion. NOT by proof.** Every testable mechanism has been tried:
- Entry alternatives: ATR filter, volume confirmation, correlation filter, Donchian, EMA crossover, CTREND, A/D — all rejected
- Exit alternatives: vol-contingent Chandelier (uniform), asymmetric exit, ATR_EMA, chop filter — all rejected
- Position sizing: ATR-norm, trend scalar, USDT hedge, drawdown trigger — all marginal or rejected
- Portfolio: equity integration, A/D sleeve, Donchian sleeve, CTREND sleeve — all rejected

**The research loop is not closed because we haven't found the right idea — it's closed because all testable ideas have been tested and rejected. Only genuinely novel mechanisms (RAE) or infrastructure (T38/T36/T37) remain.**

**Remaining paths:**
1. **Live testnet** (BLOCKED 4+ weeks on Noah's API keys — the only path that produces real feedback)
2. **T36/T38 infrastructure** (fixes the credibility of what we already have)
3. **T37 VL confirmation** (removes noise from production params)
4. **RAE** (build once if approved — genuinely novel mechanism)

---

## Anti-Overfitting Rules (Established 2026-04-25)

1. Minimum 3-window (5.5%) improvement on OOS before accepting any param change
2. No sequential optimization on same data (EP=24 lesson)
3. Never re-run confirmed params at higher resolution on the same harness (VL=96 lesson)
4. Held-out validation required for marginal wins
5. Equity curve dominance required (>80% of time bars)
6. **Absolute guardrails over relative improvement** (Donchian: +11% Sharpe but 63% pass < 69.1% guardrail → REJECTED)

---

## Key Insight: All Metrics Are Upper Bounds

Everything in HALL_OF_FAME.md is a simulation maximum. The real validation path is live testnet paper trading + comparing actual vs predicted metrics.

**Source of truth for production params: `src/live/config.rs`. Last verified: 2026-05-01.**

---