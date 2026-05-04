# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-04 15:01 UTC**

## What Changed This Session

1. **T57 Funding Rate Regime Filter: GRAVEYARD.** Dense sweep (51 thresholds × 9 universes × 10 WF windows = 4,590 runs). Pass rate NEVER improves. Best equity +12.5% at t=0.0008 is marginal/noise. Both funding-as-signal and funding-as-filter fail.
2. **T56 FEE BUG: CONFIRMED FIXED.** Re-ran progress_equity_curves.rs. Numbers match T56 fix: Turtle+Chandelier 86.9x, ATR_RANK=5 33.9x.
3. **live_compatible_wf fresh run:** 56/63 pass (88.9%), Sharpe 5.171, Base5 458.8x.

## Production Params (Frozen — 2026-05-04)

```text
EP                  = 21
TURTLE_ATR_P        = 24
TURTLE_ATR_M        = 2.0
HOLD_MAX            = 12
POSITION_CAP        = 3
ATR_ENTRY_MULT      = 0.00
REGIME_ATR_P        = 12
REGIME_LOOKBACK     = 42
ATR_RANK_THRESHOLD  = 5.0    // T=65 REJECTED held-out. T=24 REJECTED. T=5 is production.
VOL_LOOKBACK        = 96
SIZE_MULT           = 0.70   // INERT — pure risk knob
```

## Current Truth

- Live bot: Turtle-only exit + ATR_RANK=5 regime gate
- Best daily equity (dual exit, no filter): 113.6x, Sharpe 0.99
- Live config equity (dual exit, T=5): 43.1x, Sharpe 0.87
- Walk-forward (Turtle-only, T=5, VL=96): 55/63 pass, Sharpe 4.910
- All ATR rank thresholds > 5 fail held-out validation
- All Turtle params are frozen and confirmed

## Next Tasks (Priority Order)

### T56: FIX Fee Bug in progress_equity_curves.rs — COMPLETE ✔ (2026-05-04)
**Status:** FIXED (commit 1e841c23, confirmed 15:01 UTC rerun).
- Line 878 now uses `entry_px * (1.0 + TAKER_FEE)` (correct for longs)
- Corrected equity: Turtle+Chandelier 86.9x (was 113.6x), ATR_RANK=5 33.9x (was 43.1x)
- HOF, daily_progress.csv, and snapshots all updated with corrected numbers

### T55: LOB NOBI Data Collection (MEDIUM — multi-session)
**Status:** COLLECTING (aa393d7e). Daemon running since 2026-05-04 12:20 UTC.
- ✓ Collector built: `scripts/lob_collector.py` (Binance depth API, 15min interval)
- ✓ CSV output: `data/cache/lob_nobi/{btcusdt,ethusdt,solusdt}_depth.csv`
- ✓ NOBI at top-1/5/20 levels + mid price + spread
- ⚠️ Only 6 data points per symbol so far (~90 min)
- ✗ Daemon is NOT systemd — will die on reboot. Need to make persistent.
- ✗ `depth_imbalance_pipeline.rs` is a FAKE STUB (6-line hardcoded print). Needs real code.
- ✗ Need 2+ weeks of data before signal testing
- **Realistic timeline:** 4-6 weeks to first LOB signal test
- **Why:** Genuinely novel edge (arxiv 2602.00776). But honest about timeline.

### T57: Funding Rate Regime Filter — GRAVEYARD ✘ (2026-05-04)
**Status:** TESTED AND REJECTED. Dense sweep (51 thresholds × 9 universes × 10 WF windows = 4,590 runs).
- Pass rate NEVER improves at any threshold (always ≤67/90 vs 67/90 baseline)
- Best equity +12.5% at t=0.0008 — blocking only 274/2849 entries (9.6%). Marginal, likely noise.
- Aggressive thresholds (t<0.0005) destroy performance via trade starvation
- **Mechanism doesn't work:** Funding rate level doesn't predict Turtle breakout entry quality.
- **Meta-lesson:** Both funding-as-signal (prior GRAVEYARD) and funding-as-filter (T57) fail. Funding rate is not useful for this strategy class.

### COMPLETED

#### T53: Mock Exchange Bypass — COMPLETE ✔
**Status:** State machine VALIDATED (7b94003a). Only testnet API keys remain.
- ✓ MockExchange API smoke test passing (32fe20c4)
- ✓ 13 round-trip trades on real BTC data
- ✓ **LiveBot state-machine simulation: 275 trades, 5 symbols, realistic fills** (7b94003a)
- ✓ ATR-rank filter corrected to match bot.rs (ATR-as-pct-of-price)
- ✗ Live testnet BLOCKED on Noah's Binance testnet API keys

#### T54: ATR_RANK=5 Equity Run — COMPLETE ✔ (but numbers SUSPECT due to T56 fee bug)
- Dual exit (progress_equity_curves): 43.1x, Sharpe 0.87 — ⚠️ OVERSTATED (fee bug)
- Turtle-only walk-forward (live_compatible_wf, T=5, VL=96): 55/63 pass, Sharpe 4.910 — ✓ CORRECT fees

### Track C Pipeline (LOW — vapor)
- ETF flow institutional signal — zero code, zero data, 2+ weeks mentioned
- DXY-Realized-Vol regime gate — zero code
- Stablecoin exchange reserve state — zero code
- **Honest assessment:** These are aspirational ideas, not planned work. Deprioritize until T56/T55/T57 are done.

## Anti-Spin Rules

1. **ATR_RANK filter is SETTLED.** T=0/5 only. No more sweeps.
2. **All Turtle params are FROZEN.** No more hyperopts unless genuinely new mechanism.
3. **⚠️ HOF equity numbers are SUSPECT.** progress_equity_curves.rs has a fee sign error (T56). Do NOT cite 113.6x or 43.1x until T56 is fixed and rerun.
4. **If blocked on credentials, say so plainly and build the workaround (T53 — DONE).**
5. **Max 2 sequential optimizations per harness before mandatory held-out validation.**
6. **Do NOT re-confirm settled parameters.** EM=0.00 was confirmed 3 times. Stop.
7. **Walk-forward Sharpe ≠ daily equity Sharpe.** Never compare 4.91 WF avg to 0.87 daily. They measure different things.

## Graveyard Summary (Latest)

| Strategy | Result | Key Reason |
|---|---|---|
| **Funding Rate Regime Filter (T57)** | **GRAVEYARD** | **Pass rate never improves. Best equity +12.5% at t=0.0008 is marginal/noise. 4,590 runs.** |
| ATR_RANK=65 | GRAVEYARD | Held-out: 10/22 pass, Sharpe -1.900. Worse than T=24. Trade starvation. |
| ATR_RANK=24 | GRAVEYARD | Held-out: 10/22 pass, Sharpe -0.964. Same-harness artifact. |
| Short-side sleeve | GRAVEYARD | 37.5% pass vs 69.1% guardrail |
| SIZE_MULT overlay | INERT | Pure risk knob, no alpha |
| EP=24 | REVERTED | Held-out: 25/29 vs 27/29 |
| AP=64 | REJECTED | Sequential optimization pattern |

## Remaining Blocker

Live testnet (Noah's API keys, 5+ weeks). T53 mock exchange bypasses this.
