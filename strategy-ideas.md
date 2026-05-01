# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-01 09:57 UTC. Critique cycle complete. T38 (corrected live-path revalidation) is the single most critical unbuilt task. VOL_LOOKBACK VL=96 flagged as same-harness artifact (EP=24 pattern). Live testnet BLOCKED 4+ weeks.*

---

## Critical Alert: Same-Harness Artifact Pattern (EP=24/VL)

**EP=24 (REVERTED):** Found on the same harness (EP sweep) during the same session that validated CHAND_P=11. Sequential optimization on same OOS data → in-sample inflation. Reverted after held-out confirmed EP=21 wins.

**VOL_LOOKBACK VL=96 (UNCONFIRMED):** Found on the same 100-value sweep harness (VL=1..100 step 1, 9 universes × 6 windows) that produced VL=8 on 2026-04-29. One day later, VL=96 was "found" using identical methodology. This is the same pattern as EP=24.

**Rule:** Never re-run confirmed params at higher resolution on the same harness. VL=8 was settled. VL=96 is a same-harness artifact risk. **Do not promote VL=96 to production without Base5-only confirmation.**

---

## Critical Alert: All Turtle-Only Metrics Are Stale (2026-05-01)

**Bug fix (commit `79a3442d`):** `src/live/bot.rs` was using wrong ATR period for Turtle ATR buffer, wrong stop direction (lowest_low - ATR instead of highest_high - ATR), and not enforcing HOLD_MAX during ATR warmup. Fixed 2026-05-01 02:00 UTC.

**ALL prior Turtle-only walk-forward results are stale.** The following require re-run under corrected live-path semantics before they can be cited as production evidence:
- ATR_RANK=5 (Turtle-only validation pre-dates bug fix)
- TURTLE_ATR_PERIOD=24 (sweep results were degenerate because stop was unreachable)
- Any walk-forward that used "Turtle-only exit" as its exit mechanism

**Action required:** T38 (corrected live-path walk-forward) before citing any Turtle-only result.

---

## Top 3 Most Promising Unbuilt Ideas

### #1: T38 — Corrected Live Turtle-Only Walk-Forward (CRITICAL)
**Status:** UNBUILT. Most important unstarted task.
**Why:** All Turtle-only metrics are stale after the 2026-05-01 bug fix. Without this, nothing we say about Turtle-only performance is credible.
**What:** Build harness matching `src/live/bot.rs` exactly — Turtle-only exit, correct ATR buffer, HOLD_MAX independent of warmup. 9 universes × 6 windows. Label `LIVE_COMPATIBLE`.

### #2: Regime-Adaptive Exit (RAE) — Vol-Conditional Chandelier Multiplier
**Status:** UNBUILT. Genuinely novel mechanism.
**Why new:** Prior vol-contingent Chandelier (GRAVEYARD) tested uniform multiplier change → all configs identical. RAE hypothesizes conditional adjustment based on current ATR percentile rank may win where uniform fails.
**Mechanism:**
- High-vol regime (BTC ATR rank > 60th pct): M × 1.1 → looser stop, avoid premature stop-out in volatile trends
- Low-vol regime (BTC ATR rank < 40th pct): M × 0.9 → tighter stop, capture choppy range breaks faster
- Neutral regime: M = 2.30 (fixed baseline)
**What to build:** `examples/regime_adaptive_exit_walkforward.rs` — sweep high_vol_mult × low_vol_mult × 9 universes × 6 windows.
**Reject if:** Pass rate or Sharpe degrades vs fixed M=2.30. If no improvement: vol-conditional exit space is truly exhausted.

### #3: Live-Path Parity Audit Infrastructure
**Status:** UNBUILT. Audit infrastructure, not research.
**Why:** The gap between research harness and live bot code has caused multiple false conclusions (dual-exit attribution inflated, S6 incompatibility not caught, EP=24 in-sample inflation). We need a harness that compares research vs live-bot semantics bar-by-bar and stamps every HALL_OF_FAME entry.
**What:** `examples/live_path_parity_audit.rs` — runs both research and live-bot semantics on identical data, compares equity curves, reports `LIVE_COMPATIBLE` / `RESEARCH_ONLY` / `REQUIRES_LIVE_INTEGRATION` for every strategy.
**Value:** Prevents the next month of validating things the bot cannot trade.

---

## Unbuilt Ideas (Priority Within Category)

### Live Bot Parity / Validation
- [ ] **T38**: Corrected Turtle-only walk-forward (STALE — needs rebuild)
- [ ] **T9**: Live testnet (BLOCKED on API keys — 4+ weeks)

### Research (Genuinely Untested)
- [ ] **T40**: Regime-Adaptive Exit (RAE) — vol-conditional Chandelier multiplier
- [ ] **T37**: VL=96 vs VL=8 Base5-only comparison

### Audit / Infrastructure
- [ ] **Live-path parity audit** — stamp HALL_OF_FAME entries as LIVE_COMPATIBLE / RESEARCH_ONLY
- [ ] **Harness param sync** — `progress_equity_curves.rs` uses CHAND_PERIOD=11; config.rs has CHAND_PERIOD=7

---

## Tested and Rejected (Do Not Revisit Without New Mechanism)

| Strategy | Result | Key Reason |
|----------|--------|------------|
| ATR_ENTRY_MULT=0.94 | REJECTED | Held-out 10/18 vs baseline 11/18 |
| VOL_LOOKBACK=96 | UNCONFIRMED | Same-harness artifact (EP=24 pattern) — keep VL=8 until Base5 confirms |
| ATR_EMA [1..200] | CONFIRMED NULL | No improvement anywhere in range |
| FRESHNESS_COOLDOWN [0..70] | CONFIRMED NULL | cd=0 optimal |
| HOLD_MAX [1..100] | CONFIRMED NULL | HM=12 optimal, no benefit from longer |
| CHAND_MULT [1.5..5.0] | CONFIRMED NULL | M=2.30 optimal |
| ATR_ENTRY_MULT [0.00..2.00] step 0.01 | CONFIRMED NULL | EM=0.00 wins |
| S6 close_losers I=5 | GRAVEYARD | 0 trades on Turtle-only (incompatible with live bot) |
| Donchian sleeve | REJECTED | 63% < 69.1% guardrail |
| ATR-norm position sizing | REJECTED | Equal capital optimal |
| Asymmetric exit | REJECTED | All configs identical |
| Mid-caps | REJECTED | 60% < 70% threshold |
| 4h Multi-Timeframe Turtle | GRAVEYARD | 1/20 pass — structural timeframe incompatibility |
| BollingerReversion | GRAVEYARD | 0/288 OOS — signal actively harmful |
| Position scaling overlays | GRAVEYARD | All failed — Chandelier already manages it |

---

## Research Loop Status

**CLOSED by exhaustion. NOT by proof.** Every testable mechanism has been tried:
- Entry alternatives: ATR filter, volume confirmation, correlation filter, Donchian, EMA crossover, CTREND, A/D — all rejected
- Exit alternatives: vol-contingent Chandelier, asymmetric exit, ATR_EMA, chop filter — all rejected
- Position sizing: ATR-norm, trend scalar, USDT hedge, drawdown trigger — all marginal or rejected
- Portfolio: equity integration, A/D sleeve, Donchian sleeve, CTREND sleeve — all rejected

**Remaining paths:**
1. Live testnet (BLOCKED)
2. T38 revalidation (unbuilt)
3. RAE (build once if approved)
4. T37 VL confirmation (small, one harness run)

---

## Anti-Overfitting Rules (Established 2026-04-25)

1. Minimum 3-window (5.5%) improvement on OOS before accepting any param change
2. No sequential optimization on same data (EP=24 lesson)
3. Never re-run confirmed params at higher resolution on the same harness (VL=96 lesson)
4. Held-out validation required for marginal wins
5. Equity curve dominance required (>80% of time bars)
6. Absolute guardrails over relative improvement (Donchian: +11% Sharpe but 63% pass < 69.1% guardrail → REJECTED)

---

## Key Insight: All Metrics Are Upper Bounds

Everything in HALL_OF_FAME.md is a simulation maximum. The real validation path is live testnet paper trading + comparing actual vs predicted metrics.

**Source of truth for production params: `src/live/config.rs`. Last verified: 2026-05-01.**

---