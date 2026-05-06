# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-06 16:06 UTC. Major cleanup: removed stale T61 (aggTrades original), merged T61-ALT (taker-buy overlay candidate), added T80 (OOS validation), closed T70 as documentation loop.*

---

## Critical Alerts

### T61 (original, aggTrades): CLOSED — superseded by taker-buy kline data
Raw aggTrades are impractical for multi-year daily features (1000-row cap covers ~83 seconds of BTC on busy 2023 days). T76 (2026-05-06) discovered that standard Binance daily klines include `taker_buy_quote_asset_volume`. Feature cache exists at `data/cache/taker_buy/`. Original T61 definition is dead.

**New T61-ALT (execute, not document):** Build a Turtle entry + taker-buy pressure overlay candidate. Must preserve T73 top-10 winners. Benchmark vs exact-live without overlay. Promote if Sharpe improves AND top-winners preserved. Reject and close permanently if either fails.

### Documentation Loop Detected — T70 Closed
T70 (semantic gap mechanism audit) appeared in PLAN.md as "next priority" for 4+ weeks without execution. The gap is documented: research harness = 176.79x vs live bot = 2.81x. T69 patch made it worse (1.02x). T72 VL gate also made it worse (1.01x). The gap is structural and cannot be closed by a one-line parameter fix.

**Resolution:** Accept that research harness parameters (EP=21, AP=17, LB=41, VL=92) are validated on the research diagnostic system only. Live bot uses Turtle-only exit and no vol ranking. These are different systems. HOF reflects this. T70 is closed as "structural gap — not a parameter problem."

---

## Top 3 Execution Tasks (Not Documentation Updates)

### T61-ALT: Taker-Buy Pressure Overlay Candidate — EXECUTE
**Status:** T76 feature cache exists. Candidate not built.
- Use `data/cache/taker_buy/` parquet files (BTC/ETH/SOL/XRP/DOGE 2000+ daily bars).
- Candidate: Turtle breakout entry + pressure > 50th pct confirmation. Long only.
- Benchmark: exact-live without pressure overlay.
- Guardrail: must preserve T73 top-10 winners (8/10 have DV rank 4-6 — pressure gate must not exclude rank 4-6 symbols).
- Decision: improve AND preserve → promote; degrade OR kill winners → reject permanently.

### T53-RESOLUTION: State the Blocker Clearly
**Status:** "Blocked" for 5+ weeks. Do not persist another week.
- Option A (reduce scope): daily-bar mock. Wire `src/live/bot.rs` → mock → verify signals match exact-live harness. Run without 1m data.
- Option B (escalate): "Mock bypass blocked on testnet API keys. Resolution: Noah provides keys or we accept dry-run-only deployability."
- Do NOT write "blocked — revisit next session."

### T80: Out-Of-Sample Universe Validation — NEW
**Status:** Not built. Addresses the "no OOS universe" blind spot.
- Reserve UNIUSDT, MATICUSDT, AVAXUSDT as explicit hold-out universes (never optimized on).
- Run exact-live Turtle-only walk-forward: 3 pairs × 6 windows.
- Decision: ≥70% pass / Sharpe ≥0.5 → validate; <60% pass → document generalization boundary.
- Motivation: we've used the same 9-universe grid since April and may have "learned" it through repetition.

---

## Infrastructure / Trust Tasks

- [x] **T65**: Exact live-bot source-of-truth harness — DONE
- [x] **T67**: Regenerate HOF/reports from exact-live only — DONE
- [x] **T68**: Drawdown abandonment / risk-of-ruin stress — DONE
- [x] **T73**: Top-winner conditions audit — DONE (guardrail documented)
- [x] **T74**: TURTLE_ATR_MULT stale sweep closure — DONE
- [x] **T75**: HEDGE_ATR_PERIOD → 38 — PROMOTED (real improvement)
- [x] **T76**: Taker-buy pressure feature cache — DONE (data exists)
- [ ] **T61-ALT**: Taker-buy overlay candidate — UNBUILT (execute, not document)
- [ ] **T53**: Mock exchange resolution — UNBUILT (escalate or reduce scope)
- [ ] **T80**: OOS universe validation — NEW

---

## Resolved / Closed

- **T61 (aggTrades original):** DEAD — 1000-row cap impractical for multi-year; superseded by taker-buy kline data (T76).
- **T70 (semantic gap audit):** CLOSED — gap is structural, not a parameter problem. HOF reflects actual live bot semantics. Research parameters are not transferable to live path.
- **T72 VOL_LOOKBACK live gate:** REJECTED — 1.01x vs 2.56x. VL ranking excluded 8/10 top winners.
- **T74 TURTLE_ATR_MULT:** M=2.00 reconfirmed; no more nearby sweeps.
- **T75 HEDGE_ATR_PERIOD:** P=38 promoted; +0.25x live equity improvement.
- **T69 semantic alignment:** REJECTED — 1.02x (worse than live bot 2.81x).
- **T67 HEDGE_ATR_PCT:** NULL — 101 identical values, inert.
- **T67 HEDGE_SIZE_MULT:** 0.40 — pure risk dial, not alpha.
- **T76 taker-buy pressure feature:** cache built; signal mixed; candidate not yet built.

---

## New Concepts Added 2026-05-06

### C13: OOS Universe Reserve
Reserve 2-3 crypto pairs (UNI, MATIC, AVAX) as explicit hold-out never-optimized validation. Run walk-forward on them to establish generalization boundary. If they pass, we have new trust evidence. If they fail, we know the strategy is optimized to our 9-pair universe.

### C14: Execute-Or-Close Rule
Every task in PLAN.md must have a decision point that either (a) produces a committed artifact or (b) explicitly closes the issue. "Deferred to next session" is not a decision — it is a loop.

### C15: Fee-Adjusted Sharpe Range
Maker-fill assumption (70.6%) validated on one crash window (FTX). Fee drag could be 22% (if maker-fill holds) or 50% (if maker-fill degrades to 40% in sustained bear). Report fee-adjusted Sharpe as a range, not a point estimate. "Sharpe 0.8–1.3 after realistic fees" is more honest than "Sharpe 1.01."

---

## Anti-Overfitting Rules

1. No Turtle-family parameter sweeps unless a new mechanism is proposed.
2. No "audit" tasks — write the test or close the issue.
3. New entry filter must pass top-trade skip audit before promotion.
4. Exact live-path daily equity required before quoting production metrics.
5. 176.79x research harness appears in HOF once: as diagnostic output, not production performance.
6. Every task has an execute-or-close decision — no deferred loops.

---

## Sharpe Taxonomy (Required Labels)

| Type | Description | Current Value |
|------|-------------|---------------|
| Daily compounded account | Real equity from exact-live replay | **1.01** |
| Per-window walk-forward | Mean of per-window Sharpe ratios | ~5.5 (inflated) |
| Attribution | Role in portfolio context | N/A |
| Milestone-aggregated | Trade-level aggregation | Not comparable |

Never compare daily account Sharpe to per-window walk-forward Sharpe. They measure different things.