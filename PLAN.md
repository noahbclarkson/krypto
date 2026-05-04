# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-04 16:17 UTC**

## What Changed This Session

1. **CRITIQUE CYCLE (16:17 UTC).** Identified critical equity black hole: live bot (Turtle-only exit) has NEVER had compounded daily equity computed. The 33.9x figure is from the dual-exit path (wrong strategy). See `memory/2026-05-04.md` [16:17 UTC] section.
2. **T58 USDT Hedge Threshold: INERT (confirmed).** 6,363 runs. All values produce identical 706 trades — pure risk knob.
3. **T57 Funding Rate Regime Filter: GRAVEYARD.** 4,590 runs. Pass rate NEVER improves.
4. **T56 FEE BUG: CONFIRMED FIXED.**
5. **live_compatible_wf fresh run:** 55/63 pass (87.3%), Sharpe 4.910, Base5 622.98x.

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
HEDGE_PCT           = 75     // INERT — pure risk knob (T58: 101-value sweep confirms)
```

## Current Truth

- Live bot: Turtle-only exit + ATR_RANK=5 regime gate
- ⚠️ **Live-path equity: UNKNOWN.** progress_equity_curves.rs uses DUAL EXIT (Chandelier+Turtle). Live bot is Turtle-only. These are DIFFERENT strategies.
- Dual-exit daily equity (not live path): 86.9x / Sharpe 0.95 (fee-corrected T56)
- Dual-exit + ATR_RANK=5 (not live path): 33.9x / Sharpe 0.42 (fee-corrected T56)
- Walk-forward Turtle-only (T=5, VL=96): 55/63 pass, WF avg Sharpe 4.910
- **Walk-forward Sharpe (4.91) ≠ daily equity Sharpe (0.42-0.95).** Different metrics. Do not conflate.
- All ATR rank thresholds > 5 fail held-out validation
- All Turtle params are frozen and confirmed
- ⚠️ **Per-year performance decomposition: NEVER DONE.** Unknown if bull-market dependent.

## Next Tasks (Priority Order)

### T59: Turtle-Only Daily Equity Curve (IMMEDIATE)
**Status:** UNBUILT.
- **Problem:** The live bot runs Turtle-only + ATR_RANK=5. `progress_equity_curves.rs` runs Dual Exit (Chandelier+Turtle). We have NO compounded daily equity curve for the live strategy.
- **Action:** Build a Turtle-only mode for `progress_equity_curves.rs` that exactly matches `src/live/bot.rs` (using `check_turtle_exit` logic).
- **Why:** Every reporting metric for production is currently using the wrong strategy.

### T60: Per-Year Performance Decomposition (IMMEDIATE)
**Status:** UNBUILT.
- **Problem:** Unknown bull market bias. Walk-forward Sharpe averages per-window metrics, masking multi-year drawdowns.
- **Action:** Decompose the (new) Turtle-only daily equity curve by calendar year (2020-2026).
- **Output:** Report Sharpe, MaxDD, Return, and Trade Count per year.
- **Why:** If the edge only exists in 2020-2021, the strategy is not robust for 2026.

### T61: Binance aggTrades Order Flow Signal (HIGH — 1-2 Sessions)
**Status:** NEW CONCEPT.
- **Problem:** LOB NOBI (T55) is 6+ weeks away from having enough data.
- **Action:** Download historical `aggTrades` from data.binance.vision. Build rolling 5-min buy/sell imbalance. Test as entry confirmation gate.
- **Why:** Genuinely novel microstructure edge that doesn't require a 6-week collection period.

### T53: Mock Exchange Bypass — WAITING ON LIVE INTEGRATION
**Status:** State machine VALIDATED (7b94003a). Only testnet API keys remain.
- ✓ MockExchange API smoke test passing (32fe20c4)
- ✓ **LiveBot state-machine simulation: 275 trades, 5 symbols, realistic fills** (7b94003a)
- ✗ Live testnet BLOCKED on Noah's Binance testnet API keys

### T55: LOB NOBI Data Collection (LOW — multi-week background task)
**Status:** COLLECTING (aa393d7e). Daemon running since 2026-05-04 12:20 UTC.
- ✓ CSV output: `data/cache/lob_nobi/` (currently only a few hours of data)
- ✗ Need 2+ weeks of data before signal testing. Deprioritized for active research until data matures.

### COMPLETED

#### T58: USDT Hedge Threshold Extensive Hyperopt — COMPLETE ✔ (2026-05-04)
| ATR_RANK=24 | GRAVEYARD | Held-out: 10/22 pass, Sharpe -0.964. Same-harness artifact. |
| Short-side sleeve | GRAVEYARD | 37.5% pass vs 69.1% guardrail |
| SIZE_MULT overlay | INERT | Pure risk knob, no alpha |
| EP=24 | REVERTED | Held-out: 25/29 vs 27/29 |
| AP=64 | REJECTED | Sequential optimization pattern |

## Remaining Blocker

Live testnet (Noah's API keys, 5+ weeks). T53 mock exchange bypasses this.
