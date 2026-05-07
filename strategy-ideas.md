# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-07 08:05 UTC. C16/C17/C18 resolved. C19 rebalancing overlay added. T80 result: genuine generalization failure.*

---

## Critical Alerts

### C16: CLOSED PERMANENTLY (2026-05-07)
CHAND_PERIOD 98-value extensive sweep proved all 98 values produce IDENTICAL results (Sharpe=2.797, pass=68.3%, equity=1.4538x). Root cause: Turtle ATR exit fires before Chandelier in dual-exit architecture. Chandelier is non-binding in dual-exit. Modulating a non-binding exit has zero effect. **Remove from active list permanently.**

### C17: CLOSED PERMANENTLY (2026-05-07)
C17 consecutive-bar filter REJECTED: 81% vs 93.7% pass, -2.9% equity delta. The dual-gate decision rule requires both metrics to improve. Fails. **Remove from active list permanently.**

### C18: ACCEPTABLE — CLOSED (2026-05-07)
Maker-fill rate stress test: equity 2.808x at 40% maker fill, Sharpe 0.841, MaxDD 28.1%. Equity range 2.80x–2.85x across maker_fill ∈ [30%, 100%] — low sensitivity. **Remove from active list permanently.**

### T80: OOS Universe Validation — RESULT: GENERALIZATION FAILURE
Built and executed on hold-out UNI/MATIC/AVAX:
- **11/18 pass (61.1%)**, avg Sharpe **0.149**, avg return **+2.3%/window**, 160 trades
- MATIC strong (6/6), AVAX borderline (4/6), UNI catastrophic (1/6)
- **Fails promotion guardrail** (≥70% pass + Sharpe ≥0.5): 9pp below pass threshold, 0.35 below Sharpe threshold
- **Honest statement:** edge is universe-sensitive, concentrated in high-beta trending pairs; does NOT generalize cleanly to unseen pairs
- UNI failure is a real signal, not noise: mature high-cap pairs with different trend dynamics fail

---

## New Concepts (2026-05-07 Session)

### C19: Rebalancing Overlay — Exact-Live Verification Required
**Mechanism:** `close_losers interval=5` — close and reopen a position when it drifts >X% from entry relative to the portfolio. Specifically targets "ugly regime" positions that Chandelier hasn't exited yet.

**Sweep result (live_compatible_wf harness):** close_losers I=5: **6/6 pass**, Sharpe **+7.527** vs baseline **+4.381** (**+3.146 delta**), avg return +74.7%, 122 trades. WINNER of the S6 rebalancing sweep.

**NOT yet tested on exact-live path.** The rebalancing was tested in `live_compatible_wf`, which has different semantics from `src/live/bot.rs`. Previous candidates (T72, T69) passed their harness tests but failed exact-live semantics. This MUST be verified on exact-live path before any promotion.

**Test:** Replay exact-live Turtle path (2.89x / Sharpe 0.98 / 298 trades) with close_losers I=5 overlay. Compare equity and Sharpe vs baseline.

**Decision:** Both equity AND Sharpe improve → promote to live bot exit logic. Either degrades → GRAVEYARD permanently.

### M1: Equity Trajectory Monitor
**Mechanism:** Compute 60-day rolling return of exact-live equity. Alert if rolling return falls below calibrated threshold (e.g., 10th percentile of historical rolling returns).

**Why needed:** Top-10 trades = 82.8% of log return. 2026 YTD already demonstrating tail risk (-22.7% Turtle vs +12.7% BTC). No automated detection exists. Manual audit is reactive, not proactive.

**Status:** Operational infrastructure, not research. Build it.

---

## Top Execution Tasks (Priority Order)

### C19: Rebalancing Overlay on Exact-Live Path — VERIFY OR KILL
- Replay exact-live Turtle path with close_losers I=5 overlay
- Compare equity to 2.89x / Sharpe 0.98 / 298 trades baseline
- Decision: both equity AND Sharpe improve → promote; either degrades → GRAVEYARD permanently

### M1: Equity Trajectory Monitor — BUILD
- Compute 60-day rolling return from `snapshots/live_bot_exact_equity.csv`
- Alert threshold: below 10th percentile of historical rolling returns
- Output: text alert + optional Discord notification
- No data download — reuse existing equity CSV

### T53: API Keys Escalation — NEEDS NOAH ACTION
- Signal path verified. Mock exchange smoke test passed.
- Only remaining blocker: Noah's Binance testnet API keys.
- Do NOT defer again. Send explicit request to Noah in #krypto.

---

## Infrastructure / Trust Tasks

- [x] **T65**: Exact live-bot source-of-truth harness — DONE
- [x] **T67**: Regenerate HOF/reports from exact-live only — DONE
- [x] **T68**: Drawdown abandonment / risk-of-ruin stress — DONE
- [x] **T73**: Top-winner conditions audit — DONE (guardrail documented)
- [x] **T74**: TURTLE_ATR_MULT stale sweep closure — DONE
- [x] **T75**: HEDGE_ATR_PERIOD → 38 — PROMOTED (real improvement)
- [x] **T76**: Taker-buy pressure feature cache — DONE
- [x] **T61-ALT**: Taker-buy overlay candidate — REJECTED (equity no better, 6/10 top winners)
- [x] **C16**: Regime-conditional Chandelier — CLOSED PERMANENTLY (Chandelier non-binding)
- [x] **C17**: Consecutive-bar filter — REJECTED (fails dual-gate)
- [x] **C18**: Maker-fill stress test — ACCEPTABLE (equity 2.808x at 40% fill)
- [x] **T80**: OOS universe validation — EXECUTED (generalization failure, not clean)
- [ ] **T53**: Mock exchange resolution — CLOSED, only API keys block
- [ ] **M1**: Equity trajectory monitor — UNBUILT
- [ ] **C19**: Rebalancing overlay on exact-live — UNVERIFIED (sweep promising, exact-live untested)

---

## Resolved / Closed

- **C16 (regime-conditional Chandelier):** CLOSED — CHAND_PERIOD sweep proves all values identical; Chandelier non-binding; modulating it has zero effect
- **C17 (consecutive-bar momentum filter):** REJECTED — 81% vs 93.7% pass, -2.9% equity delta; fails dual-gate decision rule
- **C18 (maker-fill stress test):** ACCEPTABLE — 40% fill = 2.808x, Sharpe 0.841; equity range 2.80x–2.85x; low sensitivity confirmed
- **T61-ALT (taker-buy pressure overlay):** REJECTED — equity no better, 6/10 top winners destroyed
- **T72 VOL_LOOKBACK live gate:** REJECTED — 1.01x vs 2.56x; killed 8/10 top winners
- **T74 TURTLE_ATR_MULT:** M=2.00 confirmed; no more nearby sweeps
- **T75 HEDGE_ATR_PERIOD:** P=38 promoted
- **T69 semantic alignment:** REJECTED — worsened exact live replay
- **T67 HEDGE_ATR_PCT:** INERT — all 101 values identical
- **T67 HEDGE_SIZE_MULT:** 0.55 — risk dial (updated)
- **ATR_ENTRY_MULT>0:** DEAD — mult=0.00 definitively optimal
- **EP=24:** REVERTED — held-out failure
- **Weekend filter:** REJECTED
- **ATR_RANK=24/65:** Failed held-out; T=5.0 is risk dial only
- **Donchian entry:** REJECTED — lower pass rate than Turtle
- **Vol-contingent Chandelier:** DEAD — too slow-moving
- **Taker-buy pressure entry overlay:** REJECTED
- **CHAND_PERIOD:** PROVED inert — 98-value sweep, all values identical

---

## Anti-Overfitting Rules

1. No Turtle-family parameter sweeps unless a new mechanism is proposed.
2. No "audit" tasks — write the test or close the issue.
3. New entry/exit mechanism must pass exact-live path before promotion (not just harness).
4. Exact live-path daily equity required before quoting production metrics.
5. 176.79x research harness appears in HOF once: as diagnostic output, not production performance.
6. Every task has an execute-or-close decision — no deferred loops.
7. Per-window walk-forward Sharpe is not comparable to daily account Sharpe — never mix them.

---

## Sharpe Taxonomy (Required Labels)

| Type | Description | Honest Value |
|------|-------------|--------------|
| Daily compounded account | Real equity from exact-live replay | **0.98** (updated) |
| Per-window walk-forward | Mean of per-window Sharpe ratios | ~5.5 (inflated ~5x, not comparable) |
| Fee-adjusted (maker-fill range) | Realistic range at 30-80% maker fill | **0.6–1.3** (estimated) |

Report fee-adjusted Sharpe as a range, not a point estimate.
