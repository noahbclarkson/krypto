# PLAN.md — Krypto Live Testnet Priority

**State: 2026-04-21 08:15 UTC. Critique session completed. TOP PRIORITY: Run 2026-only OOS validation. HALL_OF_FAME.md needs full audit. Hyperopt cycling stopped.**

---

## CRITICAL — Do First

### T-2026: Walk-Forward Including 2026 OOS Data
**This is the most important test we can run. Zero of our 54 OOS windows include 2026 data.**
- Run frozen params (P=11/M=2.25, EP=21, HM=12, ATR_ENTRY_MULT=0.90) on 2026-01-01 to 2026-04-21
- Report: Sharpe, return, maxDD, trade count, pass/fail against sh>0
- Hypothesis: All param sets (P=5/M=3.00, P=11/M=2.25, P=15/M=1.50) produce IDENTICAL results — Turtle ATR dominates
- **Status:** NEW — was idea #28 in strategy-ideas.md. MUST RUN.

### T1: Equity Curve Regeneration + Source Verification
**Progress chart has been wrong 3+ times in project history.**
- Run `cargo run --example progress_equity_curves --profile sweep`
- Manually verify column mapping: which column = turtle_equity, which = ad_equity
- Verify the equity numbers match what the Rust harness actually writes
- Only after column verification: generate PNG and send to Discord
- **Until verified:** Do NOT send equity charts to Discord

### T2: HALL_OF_FAME.md Full Audit
**HALL_OF_FAME has been stale for 4+ sessions. Contradictions across files.**
- Pull production params ONLY from `examples/live_turtle_chandelier.rs` source code
- Verify every param (EP, CHAND_P, CHAND_M, ATR_ENTRY_MULT, HOLD_MAX, etc.)
- Rewrite HALL_OF_FAME to match code exactly, add staleness disclaimer
- Add header: "WARNING: May be stale. Source of truth: live_turtle_chandelier.rs"

---

## HIGH PRIORITY

### T3: 2026 YTD Parameter Investigation
**All 3 param sets produce IDENTICAL -32.8% result in 2026 YTD. Why?**
- Re-run with EP=21 (EP=24 may be hyperopt noise)
- Check: does Turtle ATR exit fire differently at EP=21 vs EP=24 in 2026?
- Run ATR_ENTRY_MULT=0.90 on 2026-only data — does it help or hurt?
- Document: is 2026 regime-inherent loss or signal quality degradation?

### T4: CTREND-Native Exit Walk-Forward
**Hypothesis:** CTREND signal confirmed genuine, but wrong exit mechanism (Chandelier too tight).
- Test: Exit when shorter-horizon CTREND flips against position
- Sweep fixed holds: 10, 15, 21, 30, 45, 60, 90 bars
- Compare vs Turtle+Chandelier baseline (43/54 pass)
- If any CTREND variant beats 38/54 -> viable alternative signal family

### T5: Commit Quality Gate
**No more undocumented bug fix commits.**
- "Turtle ATR exit bug fix" in `6ec838bd` is unexplained
- If real signal bug: prior ATR parameter results may be invalidated -> re-run ATR_PERIOD sweep
- If trivial: don't call it "bug fix" — say "refactor: clean up ATR calculation comments"
- **Rule:** Every commit touching signal logic must describe WHAT bug and WHICH harness validated

---

## BLOCKED — Waiting on Noah

### T6: Live Testnet
Noah needs testnet API keys. Without this, no live paper trading.

---

## STOP DOING

- **Hyperopt cycling on stable params** — ATR_MULT 3x, ATR_PERIOD 3x, CHAND_MULT 3x, EP 4x. All confirm prior results. Stop.
- **Equity vanity numbers** — stop saying "$67M", "1048.5x", "670,515%". Report equity Sharpe (~1.3) and pass rate (83%).
- **HALL_OF_FAME as source of truth** — treat as potentially stale. Verify against source code.
- **Undocumented bug fix commits** — document which harness confirmed the fix.
- **Running without --profile sweep** — build speed matters.

---

## Current Production Params (CONFLICTING — T2 MUST RESOLVE)

```
EP = 24 (HALL_OF_FAME) vs 21 (ATR_ENTRY_MULT commit) ← CONFLICT
CHAND_PERIOD = 11 (HALL_OF_FAME) vs 15 (live code reported) ← CONFLICT
CHAND_MULT = 2.25 (HALL_OF_FAME) vs 1.50 (live code reported) ← CONFLICT
ATR_ENTRY_MULT = 0.90
HOLD_MAX = 12
POSITION_CAP = 3
FRESHNESS_COOLDOWN = 0
```

**HALL_OF_FAME contradicts live code. T2 audit must resolve this.**

---

## Research Loop Status

Genuinely untested ideas:
1. **#26 CTREND-native exit** — untested, promising mechanism hypothesis
2. **#27 4h multi-timeframe Turtle** — genuinely new territory (all validation is daily)
3. **#28 2026 OOS validation** — MUST RUN, not optional

All non-trend strategies: GRAVEYARD.
All regime switching: GRAVEYARD.
All entry-side filters: GRAVEYARD.

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **ZERO 2026 in walk-forward** | CRITICAL | T-2026 |
| **HALL_OF_FAME contradictions** | CRITICAL | T2 audit |
| **Equity chart unverified** | CRITICAL | T1 in progress |
| **2026 YTD -22.7% unexplained** | CRITICAL | T3 investigation |
| **Hyperopt cycling (no new knowledge)** | MODERATE | STOP DOING |
| **SOL slippage vs model (3.7x miss)** | MODERATE | Accept — tracker built |
| **Noah's testnet API keys** | BLOCKED | Waiting |
