# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-04 20:05 UTC**

## What Changed This Session

1. **CRITIQUE CYCLE (20:05 UTC).** Identified critical anti-overfit violation: `config.rs` updated to `REGIME_ATR_PERIOD=63` WITHOUT held-out validation. This is the EP=24 pattern repeating. AP=63 won by +1 window (+1.197 Sharpe, marginal) on the same OOS harness it would be validated against. Anti-overfit rule requires held-out for wins < 3 windows. See `memory/2026-05-04.md` [20:05 UTC] section.
2. **T59 Turtle-only equity: STILL UNBUILT.** Second session overdue. Live bot path equity unknown.
3. **T60 per-year decomposition: STILL UNBUILT.** Second session overdue.
4. **T62 added:** AP=12 vs AP=63 held-out validation — immediately needed to close AP question permanently.
5. **T61 (aggTrades) added to pipeline:** Genuinely novel microstructure signal. No 6-week wait required.

## ⚠️ CRITICAL: AP=63 Anti-Overfit Violation

**`src/live/config.rs` line 38 was updated to `REGIME_ATR_PERIOD = 63` without held-out validation.**

Evidence: `snapshots/ap_hyperopt.md` says in its own "Next Steps":
> "Held-out validation — test AP=63 against pre-2021 held-out data"
> "Update `config.rs` **if** AP=63 is promoted"

But config.rs was updated before held-out validation was done.

**The EP=24 pattern:**
- EP=24: won by +0.16 Sharpe on same OOS harness → failed held-out (25/29 vs 27/29)
- AP=63: won by +1 window on same OOS harness (+1.197 Sharpe, marginal)

**Fix required (one of):**
- **Option A (fast):** Revert `config.rs` to `AP=12` now; run held-out validation; promote only if it passes
- **Option B (correct):** Run held-out validation first; if AP=63 passes, keep it; if not, revert

**Do not leave AP=63 unvalidated in production.** It is currently in violation of the anti-overfit rules.

## Production Params (⚠️ AP PENDING VALIDATION — 2026-05-04)

```text
EP                  = 21
TURTLE_ATR_P        = 24
TURTLE_ATR_M        = 2.0
HOLD_MAX            = 12
POSITION_CAP        = 3
ATR_ENTRY_MULT      = 0.00
REGIME_ATR_P        = ⚠️ 63  // PREMATURELY UPDATED — needs held-out validation (T62)
REGIME_LOOKBACK     = 42
ATR_RANK_THRESHOLD  = 5.0    // T=65 REJECTED held-out. T=24 REJECTED. T=5 is production.
VOL_LOOKBACK        = 96
SIZE_MULT           = 0.70   // INERT — pure risk knob
HEDGE_PCT           = 75     // INERT — pure risk knob
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
- ⚠️ **REGIME_ATR_PERIOD=63: UNVALIDATED in production.** Same anti-overfit violation as EP=24.

## Next Tasks (Priority Order)

### T62: AP=63 Held-Out Validation (IMMEDIATE — 1 session)
**Status:** UNBUILT.
- **Problem:** AP=63 was promoted to `config.rs` without held-out validation — identical pattern to EP=24 failure.
- **Action:** Build `examples/ap63_held_out_validation.rs` testing AP=12 vs AP=63 on pre-2021 data. Use same universe split as `regime_stress_p7_validation.rs`.
- **Decision rule:** AP=63 passes held-out → keep it. AP=63 fails held-out → revert config.rs to AP=12.
- **Why:** Anti-overfit rules require held-out for marginal wins (< 3 windows over baseline). AP=63 won by +1 window. Cannot leave production in an unvalidated anti-overfit-violating state.

### T59: Turtle-Only Daily Equity Curve (IMMEDIATE)
**Status:** UNBUILT. Second session overdue.
- **Problem:** The live bot runs Turtle-only + ATR_RANK=5. `progress_equity_curves.rs` runs Dual Exit (Chandelier+Turtle). We have NO compounded daily equity curve for the live strategy.
- **Action:** Add a Turtle-only mode to `progress_equity_curves.rs` using `check_turtle_exit` logic from `src/live/bot.rs`. Export daily compounded equity CSV.
- **Output:** `snapshots/turtle_only_equity.md` with final equity, Sharpe, MaxDD, trade count.
- **Why:** Every reporting metric for production is currently using the wrong strategy.

### T60: Per-Year Performance Decomposition (IMMEDIATE)
**Status:** UNBUILT. Second session overdue.
- **Problem:** Unknown bull market bias. Walk-forward Sharpe averages per-window metrics, masking multi-year drawdowns.
- **Action:** Use the Turtle-only daily equity curve from T59. Decompose by calendar year (2020-2026).
- **Output:** Per-year: Sharpe, MaxDD, Return, Trade Count. Identify which years drive equity.
- **Why:** If the edge only exists in 2020-2021, forward expectations should be lower.

### T61: Binance aggTrades Order Flow Signal (HIGH — 1-2 Sessions)
**Status:** NEW CONCEPT.
- **Problem:** LOB NOBI (T55) is 6+ weeks away from having enough data.
- **Action:** Download historical aggTrades from `data.binance.vision`. Aggregate buy/sell imbalance over 5-min windows → rolling daily net flow → test as Turtle entry confirmation filter.
- **Why:** Genuinely novel microstructure edge that doesn't require a 6-week collection period. Order flow imbalance is a proven alpha source in TradFi.

### T53: Mock Exchange Bypass — WAITING ON LIVE INTEGRATION
**Status:** State machine VALIDATED (7b94003a). Only testnet API keys remain.
- ✓ MockExchange API smoke test passing (32fe20c4)
- ✓ **LiveBot state-machine simulation: 275 trades, 5 symbols, realistic fills** (7b94003a)
- ✗ Live testnet BLOCKED on Noah's Binance testnet API keys

### T55: LOB NOBI Data Collection (LOW — multi-week background task)
**Status:** COLLECTING. Daemon not persistent (dies on reboot).
- Need systemd unit for persistence across reboots
- Need 2+ weeks of data before signal testing
- Deprioritized for active research until data matures

## Completed This Session (20:05 UTC)

- Critique cycle completed — documented in `memory/2026-05-04.md`
- AP=63 anti-overfit violation identified and documented
- T62 held-out validation task added to PLAN.md

## COMPLETED (Historical)

#### T58: USDT Hedge Threshold Extensive Hyperopt — COMPLETE ✔ (2026-05-04)
| ATR_RANK=24 | GRAVEYARD | Held-out: 10/22 pass, Sharpe -0.964. Same-harness artifact. |
| Short-side sleeve | GRAVEYARD | 37.5% pass vs 69.1% guardrail |
| SIZE_MULT overlay | INERT | Pure risk knob, no alpha |
| EP=24 | REVERTED | Held-out: 25/29 vs 27/29 |
| AP=64 | REJECTED | Sequential optimization pattern |

#### T57: Funding Rate Regime Filter — COMPLETE ✔ (2026-05-04)
4,590 runs. Pass rate NEVER improves at any threshold. GRAVEYARD.

## Remaining Blocker

Live testnet (Noah's API keys, 5+ weeks). T53 mock exchange bypasses this.
