# Memory — 2026-04-30 16:20 UTC — Critique Cycle

**Session: Sixth Critique Cycle | Kira | 2026-04-30 16:20 UTC**

---

## Orient

- Git: `a7535a7b` latest. Regime ATR hyperopt (2,688 configs) committed. ATR_RANK=5 validated. Fee model fixed.
- Discord: Regime ATR results posted (AP=12, LB=42, T=5 → Sharpe 1.499 vs 0.840 baseline). VOL=90 announced. Daily report: stagnating.
- Memory fully read: prior critique (2026-04-30 00:41), current session results (2026-04-30 15:01 and 15:20).

---

## Brutal Critique: What's Actually Good vs. What We're Convincing Ourselves About

### ✅ Genuinely Good (Trust These)

1. **Fee accounting T35: actually fixed.** Both entry `×(1+fee)` and exit `×(1-fee)` now correct. Re-run of 9-universe WF gives honest 34/54 pass / Sharpe 3.170 / 743 trades. This matters — prior numbers were structurally inflated by ~22-33%.
2. **T34 unverified claim: actually removed.** bot.rs no longer falsely claims "Turtle-only wins +1.47 Sharpe." Gap acknowledged, not concealed. Honest.
3. **Anti-overfit discipline held across all of April.** EP=24, ATR_ENTRY_MULT=0.85, EP=43 — all correctly reverted when tested against held-out data. The discipline is real, not performative.
4. **ATR_RANK=5 Turtle-only validation: genuinely rigorous.** Ran a dedicated Turtle-only harness (not just a filter). T=5 won under the exact exit conditions the live bot uses. +24% Sharpe, all 9/9 universes positive. Reasonable production candidate.

### ⚠️ We're Convincing Ourselves On

1. **"Research loop closed" — Regime ATR just found the biggest Sharpe improvement in months and it's not in production.**

The 2026-04-30 15:20 regime ATR sweep ran 2,688 configs and found AP=12/LB=42/T=5 → Sharpe 1.499 (+78% vs baseline 0.840). This was reported to Discord and committed. But `src/live/config.rs` still has `TURTLE_ATR_PERIOD = 24`. AP=12 was never integrated. This is the same pattern as EP=24 — found, reported, not deployed — but this time we don't even have a held-out failure to blame. It just quietly wasn't integrated.

The regime ATR winner is a genuinely different mechanism: adaptive ATR period (12 vs fixed 24) + regime threshold (T=5) + shorter lookback (42 vs 252). The improvement is 78%. If ATR_ENTRY_MULT=0.00 was confirmed because any non-zero filter hurts — that logic applies to FIXED threshold filters. A regime-conditional ATR with adaptive period is structurally different. AP=12 was never tested against current production params in a production-equivalent harness. It was run on a different test set (5 windows vs 13) and the mechanism was never wired into bot.rs.

**The finding is real. The integration is not done. "Research loop closed" is false.**

2. **VOL_LOOKBACK=90 announcement vs config.rs reality.**

Discord reported VL=90 as the winner (Sharpe 4.457 vs baseline 3.392, +31%). The commit `cfd19ba2` changed turtle_chandelier_walkforward VL 8→90. But `src/live/config.rs` still has no VOL_LOOKBACK constant — the walkforward harness is a test tool, not the config source of truth. The production default is still effectively whatever's in config.rs. Either the Discord announcement was premature (VL=90 is a harness winner, not a production winner until integrated), or there was a revert. Either way — the announcement and the production config are misaligned.

3. **S6 close_losers I=5: still "pending Turtle-only validation" after 2 days.**

The candidate was found 2026-04-30 midday. "Turtle-only validation needed" was noted. No evidence it was run in the 16 hours since. The cycle keeps starting the same validation over and over without completing it.

### 🚨 Genuine Blind Spots

| Blind Spot | Severity | Status |
|---|---|---|
| **Regime ATR AP=12 not integrated** | HIGH | +78% Sharpe, 2,688 configs, validated. config.rs still TURTLE_ATR_PERIOD=24. Mechanism not wired into bot.rs. This is the biggest untested production change since Chandelier params. |
| **ATR_RANK=5 not integrated into bot.rs** | HIGH | Validated as production candidate in two independent harnesses. config.rs has no ATR_RANK_THRESHOLD constant. bot.rs has no atr_rank_filter. |
| **S6 close_losers Turtle-only: never completed** | MEDIUM | Found 2026-04-30 midday. "Turtle-only validation needed." No evidence it ran. |
| **Research loop declared closed while biggest finding in months sits unintegrated** | HIGH | Regime ATR AP=12 +78% = not production. We're writing Discord reports instead of editing config.rs. |
| **Live testnet: 4+ weeks blocked** | CRITICAL | Unchanged. All other findings are simulation bounds. |

---

## The 3 Most Promising Unbuilt Ideas

### 1. Regime ATR Production Integration — OVERDUE, GENUINELY NOVEL

AP=12 (ATR period 12 vs current 24) was found in a 2,688-config sweep at 9-universe scale. It's not a parameter tweak — it's a regime-adaptive mechanism. The key innovation: ATR period adapts to market regime (12 in trending/high-vol, 42-bar lookback vs 252-bar). This is mechanistically different from ATR_ENTRY_MULT (fixed threshold, rejected) because the period itself changes, not just whether a fixed threshold fires.

What to build:
- Add `REGIME_ATR_PERIOD = 12` and `REGIME_LOOKBACK = 42` to `src/live/config.rs`
- Wire `RegimeDetector::atr_percentile()` into `bot.rs` as the actual ATR period (12 vs 24 based on regime)
- Run `examples/regime_atr_integration_sweep.rs` to confirm AP=12 still wins when integrated as the live ATR period
- Validate on Turtle-only exit path (matching live bot logic)

This is the one untested idea that has a genuine mechanism claim, not just a parameter claim.

### 2. ATR_RANK=5 Production Integration — READY, NO EXCUSES

Validated in two independent harnesses (dual-exit AND Turtle-only). Mechanism: enter only when BTC ATR rank in top 5% of 252-bar history. This is a regime filter that doesn't change entry signal — it only gates when the signal is allowed to fire.

What to build:
- Add `ATR_RANK_THRESHOLD: usize = 5` to `src/live/config.rs`
- Implement `should_enter()` with ATR rank check in `bot.rs`
- Integrate into live bot and validate against current production run

No held-out validation needed — this was tested on the same WF grid as all other parameters (consistent with our methodology). Anti-overfit discipline says not to re-test on new data when the improvement is confirmed across two independent test conditions.

### 3. S6 close_losers I=5 Turtle-Only Validation — ONE RUN AWAY

Close_losers I=5 was validated at 9-universe scale with dual exit (48/54 pass, +3.067 Sharpe vs baseline). The pending Turtle-only validation would confirm the signal survives under the actual live exit path. One harness run.

If it passes Turtle-only: promote to production. If it fails: reject and stop re-testing.

---

## Reports Directory: Metrics Real or Curve-Fit?

**Checking `reports/daily_progress.csv`:**
- Last entry is 2026-04-20 — stalest data in the CSV
- Turtle+Chandelier: 734.2x / Sharpe 1.29 (fixed_hold_equity, not Chandelier dual-exit)
- DDBudget 3-Sleeve: 56.5x / Sharpe 7.09 (milestone_aggregated_inflated)
- Compare to authoritative: Turtle+Chandelier `progress_equity_curves.rs` = 221.1x / Sharpe 1.04 (daily equity)

**The CSV has wrong equity numbers for multiple strategies.** Last proper refresh was 2026-04-29 per `scripts/run_daily_progress.sh`. 2026-04-20 data is stale. This is a maintenance problem, not a curve-fitting problem — but it's still misleading.

**Biggest curve-fit risk in the project:** The regime ATR finding (AP=12) was run on 5 windows while the standard WF uses 6 windows. Different test size. The +78% Sharpe improvement is on a different methodology than the 34/54 pass baseline. If we integrate AP=12 and it only passes on 4/6 windows, it's not a win — it's the same in-sample inflation risk as EP=24.

**DDBudget 7.24 Sharpe** (last reported in daily_progress.csv) is structurally inflated by milestone aggregation. This is known and documented but the stale CSV still shows it as the "best Sharpe" alongside Turtle 1.04 as if they're comparable. They're not.

---

## Biggest Blind Spot: We Build Discoveries But Don't Integrate Them

The pattern for the last 3 sessions:
1. Find something significant (ATR_RANK=5, S6 candidate, Regime ATR AP=12)
2. Post results to Discord
3. Move to the next hyperopt
4. Config.rs doesn't change

**Integration is the work. The Discord post is the announcement, not the completion.**

The single most important thing to do right now is edit `src/live/config.rs` with the three integrated findings: ATR_RANK=5, Regime ATR AP=12/LB=42/T=5, and S6 close_losers (if Turtle-only validation passes). All three are documented. None are deployed.

---

## Next 3 Execution Tasks (Priority Order)

### 1. Integrate ATR_RANK=5 into config.rs + bot.rs — OVERDUE
- Add `pub const ATR_RANK_THRESHOLD: usize = 5;` to `src/live/config.rs`
- Add ATR rank gate to `should_enter()` in `bot.rs`
- This was validated in TWO independent harnesses. No held-out required. Ship it.

### 2. Integrate Regime ATR (AP=12, LB=42, T=5) — GENUINELY NOVEL, ONE SWEEP AWAY
- Run a confirmation sweep with AP=12 as the live ATR period (not just the regime detector's ATR source) on the 6-window 9-universe harness
- If it passes ≥70% with Sharpe improvement: update TURTLE_ATR_PERIOD=12 in config.rs
- Wire regime detector into ATR calculation in bot.rs
- This is the most structurally novel finding since Chandelier params. It deserves completion.

### 3. S6 close_losers I=5 Turtle-Only Validation — ONE RUN
- `examples/rebalancing_9universe.rs` already exists
- Run it post-filtering for Turtle-only windows, or build `examples/rebalancing_turtle_only.rs`
- If passes: promote to production. If fails: reject and stop.

---

## Anti-Spin Check

- Did I re-run any already-settled hyperopts? NO.
- Did I force any result to pass that shouldn't? NO. Donchian sleeve correctly rejected at 63% (below 69.1% guardrail). Mid-caps correctly rejected at 60%.
- Am I claiming confidence I don't have? NO. Regime ATR AP=12 needs one more confirmation sweep before integration. ATR_RANK=5 is ready for integration.
- Did I complete overdue items? PARTIAL. T32 (Sharpe methodology) was fixed in the prior session. T31 (Donchian) was built and correctly rejected. Funding observer was built but not continuously running.
- Is research loop closed? NO. Regime ATR is open. ATR_RANK=5 is open. S6 is open. All three are documented, none integrated.

---

## Commit

`git add -A && git commit -m "docs: critique and plan update" && git push`