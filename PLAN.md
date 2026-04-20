# PLAN.md — Krypto Live Testnet Priority

**State: 2026-04-20 12:23 UTC. Pre-2021 test DONE (P=5/M=3.00 PASS 21/21). ATR_ENTRY=0.20 REVERTED (same session). CRITICAL gap: paired pre-2021 comparison P=5 vs P=15 NOT DONE. 2026 YTD window test NOT DONE.**

---

## 🚨 CRITICAL — P=5/M=3.00 Pre-2021: DONE. Next: PAIRED Comparison Required

**Status: 21/21 PASS, but ATR_ENTRY=0.20 showed SAME pattern and was REVERTED.**

Pre-2021 stress test for P=5/M=3.00 passed (commit 5dc295e9, 21/21). BUT: ATR_ENTRY=0.20 also passed pre-2021 (21/21) and was immediately reverted because Sharpe in pre-2021 was WORSE with the entry filter (0.32 with vs 0.36 without). P=5/M=3.00 shows the same "higher Sharpe, lower robustness" pattern as ATR_ENTRY=0.20 — higher equity, lower pass rate. We need the paired comparison.

**Required (do this first):**
```bash
# Run original P=28/M=2.0 regime test alongside P=5/M=3.00 test
# Compare pre-2021 Sharpe for both on identical windows
# regime_stress_test.rs (original P=28) vs regime_stress_p5m3.rs (P=5)
# If P=5/M=3.00 pre-2021 Sharpe < P=28/M=2.0 Sharpe → revert to P=15/M=1.50
```

**Decision tree:**
- P=5/M=3.00 pre-2021 Sharpe ≥ P=28/M=2.0 baseline → KEEP
- P=5/M=3.00 pre-2021 Sharpe < P=28/M=2.0 baseline → REVERT to P=15/M=1.50

---

## 🚨 CRITICAL — 2026 YTD Window Test (P=5 vs P=15)

**Hypothesis:** P=5/M=3.00's tighter Chandelier (fires ~bar 9 vs ~bar 15 for P=15) increases whipsaw in chop regimes. 2026 YTD (-22.7%, Sharpe -5.31) is the current chop regime. P=5 may be making it worse.

**Required:**
```bash
# Run walk-forward on the 2026 YTD window specifically
# Compare P=5/M=3.00 vs P=15/M=1.50 on this window
# If P=5 loses badly → revert immediately
```

**Until tested:** Do NOT rely on P=5/M=3.00 for current regime. P=15/M=1.50 is the conservative fallback.

---

## 🚨 CRITICAL — HALL_OF_FAME.md Audit vs config.rs

**Problem:** Commit 5dc295e9 reverted ATR_ENTRY_MULT to 0.00 but HALL_OF_FAME.md was NOT updated. It still says `ATR_ENTRY_MULT = 0.20`. This is the same drift problem we've had before (P=15/M=2.25 in HALL_OF_FAME vs P=5/M=3.00 in live code).

**Required:**
```bash
# Diff HALL_OF_FAME params against src/live/config.rs line-by-line
# Fix ALL mismatches
# ATR_ENTRY_MULT should be 0.00 (REVERTED this session)
```

---

## 📋 TASK 2: Live Slippage Tracker (#24)

**Concept:** Hook FillLog CSV logger into live_turtle_chandelier.rs. SOL slippage is the biggest known live risk (3.7x model at $100K). Build logger before live = calibrate from day 1.

**Implementation:** The FillLog struct exists but CSV per-fill logging hasn't been connected. Add to live bot. Even dry-run produces enough fills to calibrate.

**Priority:** Medium. Do before live testnet connection.

---

## 📋 TASK 3: CTREND + Chandelier Exit Walk-Forward (#23)

**Concept:** CTREND signal confirmed genuine by Monte Carlo (0/500 shuffled beat real). The exit mechanism (fixed 21-bar hold) is the flaw. Test CTREND entry + Chandelier dual-exit in walk-forward harness.

**If pass rate > CTREND fixed-hold baseline:** CTREND promoted to production signal family candidate.

**Priority:** Low — curiosity only. Live testnet is the real priority.

---

## 📋 TASK 4: 4h Multi-Timeframe Turtle Walk-Forward (#25)

**Concept:** Turtle+Chandelier on 4h bars. More trades, finer entries. Different from failed 4h MR (mean-reversion vs trend-following).

**Implementation:** Fetch 4h OHLCV from loader.rs. Adapt turtle_chandelier_walkforward.rs for 4h. Sweep EP and CHAND_PERIOD independently (cannot port daily params).

**Priority:** Low-medium. Do after Tasks 1-3.

---

## 🚫 STOP DOING

- No more equity chart posting until equity CSV is re-run and column mapping manually verified
- No more parameter changes without paired pre-2021 comparison (P=5 vs P=15 on same windows)
- No more ATR_ENTRY_MULT — REVERTED, 0.00 is production
- No more research on non-trend strategies — all GRAVEYARD'd

---

## Equity Numbers — QUOTE ONLY "HUNDREDS OF TIMES"

| Value | Source | Date | Status |
|-------|--------|------|--------|
| 1048.5x | P=5/M=3.00 full-history | 2026-04-20 | UNVERIFIED |
| 724.4x | P=15/M=2.25 | 2026-04-20 | STALE |
| 672.7x | P=15/M=1.50 | 2026-04-19 | UNVERIFIED |
| 1126x | Chandelier equity (stale) | 2026-04-15 | WRONG DATA |
| 670x | $10K→$67M | 2026-04-15 | APPROXIMATE |

**Until a clean run with verified column mapping: quote "hundreds of times" only.**

---

## Production Params (DE-FACTO — P=5/M=3.00, pending paired pre-2021 comparison)

```
EP = 21, ATR_PERIOD = 24, ATR_MULT = 0.0 (no entry filter)
CHAND_PERIOD = 5, CHAND_MULT = 3.00  ← P=5/M=3.00 PENDING paired comparison
ATR_ENTRY_MULT = 0.00 (REVERTED this session)
HOLD_MAX = 45, POSITION_CAP = 3, FRESHNESS_COOLDOWN = 0
MAX_SOL_POSITION = $50K notional
```

*Last updated: 2026-04-20 12:23 UTC — ATR_ENTRY_MULT=0.20 REVERTED. Paired pre-2021 comparison P=5 vs P=15 is #1 priority.*
