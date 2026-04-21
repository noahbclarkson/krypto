# PLAN.md — Krypto Live Testnet Priority

**State: 2026-04-21 08:13 UTC. Critique session completed. TOP PRIORITY: Run 2026-only OOS validation (T-2026). HALL_OF_FAME.md needs full audit. Hyperopt cycling to stop.**

---

## 🔴 CRITICAL — Do First

### T-2026: Walk-Forward Including 2026 OOS Data
**This is the most important test we can run. Zero of our 54 OOS windows include 2026 data.**
- Run frozen params (P=11/M=2.25, EP=21, HM=12, ATR_ENTRY_MULT=0.90) on 2026-01-01 to 2026-04-21
- Report: Sharpe, return, maxDD, trade count, pass/fail against sh>0
- Hypothesis to test: Does ANY parameter set do better than -22.7% in 2026? (P=5/M=3.00, P=11/M=2.25, P=15/M=1.50)
- This is our only genuine OOS test in the current regime. No more excuses — run it and document.
- **Status:** NEW — was idea #28 in strategy-ideas.md

---

## 🔴 HIGH PRIORITY — Honest Validation

### T1: Equity Curve Regeneration + Source Verification
**Progress chart has been wrong 3+ times in project history.**
- Run `cargo run --example progress_equity_curves --profile sweep`
- Manually verify column mapping: which column = turtle_equity, which = ad_equity, etc.
- Verify the equity numbers match what the Rust harness actually writes
- Only after column verification: generate PNG and send to Discord
- **Until verified:** Do NOT send equity charts to Discord

### T2: HALL_OF_FAME.md Full Audit
**HALL_OF_FAME has been stale for 4+ sessions. Contradictions within same commit.**
- Pull production params ONLY from `examples/live_turtle_chandelier.rs` source code
- Verify every param (EP, CHAND_P, CHAND_M, ATR_ENTRY_MULT, HOLD_MAX, etc.)
- Rewrite HALL_OF_FAME to match code exactly
- Add disclaimer: "HALL_OF_FAME may be stale — verify against live_turtle_chandelier.rs source"
- Consider: generate HALL_OF_FAME from a cargo build script

### T3: 2026 YTD Parameter Investigation
**All 3 param sets produce IDENTICAL -32.8% result in 2026 YTD. Why?**
- Re-run with EP=21 (not EP=24, since EP=24 may be noise)
- Check: does Turtle ATR exit fire differently at EP=21 vs EP=24 in 2026 regime?
- Run ATR_ENTRY_MULT=0.90 on 2026-only data — does it help or hurt?
- Document whether 2026 is regime-inherent loss or signal quality degradation

---

## 🟡 MEDIUM PRIORITY

### T4: CTREND-Native Exit Walk-Forward (Idea #26)
**Hypothesis:** CTREND signal confirmed genuine, but wrong exit mechanism (Chandelier too tight).
- Test: Exit when shorter-horizon CTREND flips against position
- Sweep fixed holds: 10, 15, 21, 30, 45, 60, 90 bars
- Compare vs Turtle+Chandelier baseline (43/54 pass)
- If any CTREND variant beats 38/54 → viable alternative signal family

### T5: Commit Quality Gate
**No more undocumented bug fix commits.**
- "Turtle ATR exit bug fix" in `6ec838bd` is unexplained — was it real signal bug or trivial?
- If real: prior ATR parameter results may be invalidated → re-run ATR_PERIOD sweep
- If trivial: don't call it a "bug fix" — say "refactor: clean up ATR calculation comments"
- **Rule:** Every commit touching signal logic must describe WHAT bug and WHICH harness validated the fix

---

## 🟢 LOW PRIORITY / WAITING ON API KEYS

### T6: Live Testnet (Blocked)
Noah needs testnet API keys. Without this, no live paper trading validation.

---

## 🚫 STOP DOING

- **Hyperopt cycling on stable params** — ATR_MULT swept 3×, ATR_PERIOD 3×, CHAND_MULT 3×, EP 4×. All confirm prior results. Stop confirming.
- **Equity vanity numbers** — stop saying "$67M", "1048.5x", "670,515%". Report equity Sharpe (~1.3) and walk-forward pass rate (83%).
- **HALL_OF_FAME as source of truth** — treat it as potentially stale narrative. Verify against source code.
- **Undocumented bug fix commits** — if it was a real signal bug, document which harness confirmed the fix.
- **Running without `--profile sweep`** — build speed matters for iteration.

---

## Current Production Params (VERIFIED FROM SOURCE — `live_turtle_chandelier.rs`)

```
EP = 24? or 21?   ← CONFLICT: HALL_OF_FAME says 24, ATR_ENTRY_MULT commit says 21
CHAND_PERIOD = 11 (HALL_OF_FAME) vs 15 (live_turtle_chandelier.rs) ← CONFLICT
CHAND_MULT = 2.25 (HALL_OF_FAME) vs 1.50 (live_turtle_chandelier.rs) ← CONFLICT
ATR_ENTRY_MULT = 0.90
HOLD_MAX = 12
POSITION_CAP = 3
FRESHNESS_COOLDOWN = 0
```

**⚠️ The production params are internally contradictory across files. T2 (HALL_OF_FAME audit) must resolve this.**

---

## Research Loop Status

Genuinely untested ideas remaining:
1. **#26 CTREND-native exit** — untested, promising mechanism hypothesis
2. **#27 4h multi-timeframe Turtle** — genuinely new territory (all validation is daily)
3. **#28 2026 OOS validation** — MUST RUN, not optional

All non-trend strategies: dead (GRAVEYARD confirmed).
All regime switching: dead.
All entry-side filters: dead.

---

## Blind Spots (Updated 2026-04-21)

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **ZERO 2026 in walk-forward** | 🔴 CRITICAL | NEW — T-2026 |
| **HALL_OF_FAME contradictions** | 🔴 CRITICAL | NEW — T2 audit |
| **Equity chart unverified** | 🔴 CRITICAL | T1 in progress |
| **2026 YTD -22.7% unexplained** | 🔴 CRITICAL | T3 investigation |
| **Hyperopt cycling (no new knowledge)** | 🟡 MODERATE | STOP DOING |
| **SOL slippage vs model (3.7x miss)** | 🟡 MODERATE | Accept — live tracker built |
| **Noah's testnet API keys** | 🔴 BLOCKED | Waiting on Noah |
