# PLAN.md — Krypto Live Testnet Priority

**State: 2026-04-21 16:05 UTC. Research CLOSED. Blocked on live testnet API keys. Equity chart stale (248x claimed, ~780x actual with P=7). EP=24 and ATR_ENTRY_MULT 0.85 are marginal noise-level changes.**

---

## CRITICAL — Do First

### T1: Equity Chart Regeneration + Column Verification
**Status: 4+ sessions overdue. The 248x number in daily_progress.csv is stale (P=15 harness).**
- `progress_equity_curves.rs` was just fixed to P=7 (76b4e6fc commit) but CSV was generated with stale P=15
- Re-run: `cargo run --example progress_equity_curves --profile sweep`
- Manually verify column mapping: print raw CSV headers → map each column to harness source → verify Python reads correct indices
- Update `reports/daily_progress.csv` with correct equity numbers
- **Until verified: Do NOT send equity charts to Discord**

### T2: CTREND + CTREND-Native Exit Walk-Forward (#26)
**Monte Carlo confirms CTREND signal is genuine (0/500 shuffled beat real). But CTREND + Chandelier exit FAILED (30/54). Exit mechanism was wrong class.**
- Hypothesis: CTREND's multi-horizon smoothing fires too slowly for Chandelier's tight stop
- Test: Sweep fixed holds (10, 15, 21, 30, 45, 60, 90 bars) + multi-horizon counter exit (exit when shorter-horizon CTREND flips against position)
- Baseline: Turtle+Chandelier = 43/54 pass
- Any CTREND variant beating 38/54 = viable signal family
- **This is the only untested idea that produces genuinely new strategy knowledge**

---

## HIGH PRIORITY

### T3: Held-Out Validation Gate for Parameter Changes
**Problem: EP=24 (+2 windows vs EP=21, +3.7% Sharpe) was accepted without held-out test. Within noise range. This is exactly how overfitting happens.**
- Rule: Any parameter change producing <+5% Sharpe improvement AND <+3 window improvement requires formal held-out test
- Held-out test: fix the parameter on W00-W03 only, validate on W04-W05 (never used in optimization)
- Apply this to EP=24 and ATR_ENTRY_MULT=0.85 before treating them as production defaults

### T4: Sideways Regime Stress Test
**All validation is bull or bear. Almost no sideways regime data. CHAND_P=7 (tightest ever used) may behave badly in low-vol chop.**
- Identify the most sideways regime in dataset: flat price + low ATR percentile
- Run Turtle+Chandelier(P=7) on that window specifically
- If P=7 fails sideways while P=11 or P=15 passes → we need a regime-conditional Chandelier parameter

---

## BLOCKED — Waiting on Noah

### T5: Live Testnet
Noah needs testnet API keys. Without this, no live paper trading. This is the only validation that matters now.

---

## STOP DOING

- **Hyperopt cycling on stable params** — EP=24 is noise, ATR_ENTRY_MULT 0.85 is marginal. No more sweeps on these.
- **Equity vanity numbers** — stop reporting 248x when actual is ~780x with current P=7. Report verified numbers only.
- **HALL_OF_FAME equity claims** — the 1048.5x was from P=5/M=3.00, doesn't match current P=7/M=2.25. Needs recalculation.
- **Treating marginal wins as confirmed production params** — EP=24, ATR_ENTRY_MULT 0.85 need held-out validation before production acceptance.

---

## Current Production Params (NEEDS HELD-OUT VALIDATION)

```
EP = 24              ← needs held-out test (was EP=21, +2 windows noise)
CHAND_PERIOD = 7     ← validated on current params
CHAND_MULT = 2.25    ← validated
ATR_ENTRY_MULT = 0.85 ← needs held-out test (+1 window, marginal)
HOLD_MAX = 12        ← validated
ATR_PERIOD = 24      ← validated
POSITION_CAP = 3     ← validated
FRESHNESS_COOLDOWN = 0 ← validated
```

---

## Research Loop Status

Genuinely untested ideas:
1. **#26 CTREND + CTREND-native exit** — Monte Carlo confirmed genuine signal, wrong exit mechanism. This is new signal family territory.
2. **#27 4h multi-timeframe Turtle** — all validation is daily, different resolution may catch short-cycle edges
3. **#30 Equity chart verification** — infrastructure built, needs clean run

All non-trend strategies: GRAVEYARD.
All regime switching: GRAVEYARD.
All entry-side filters: GRAVEYARD.

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **EP=24 needs held-out validation** | CRITICAL | T3 in progress |
| **ATR_ENTRY_MULT 0.85 needs held-out validation** | CRITICAL | T3 in progress |
| **Equity chart unverified (4+ sessions)** | CRITICAL | T1 in progress |
| **No sideways regime validation** | MODERATE | T4 in progress |
| **Live execution never validated** | BLOCKED | Waiting on T5 |
| **HALL_OF_FAME equity claim stale** | LOW | Will self-correct when T1 done |