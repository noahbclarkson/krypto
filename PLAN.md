# PLAN.md — Krypto Research and Execution Plan

**Updated: 2026-05-12 09:35 UTC — T100 Replay Readiness Complete**

---

## Current State

Research coma is broken and the two critical misrepresentation/validation gaps are closed.

- **T99 Turtle-only pre-2021 validation:** PASS — 5/5 symbols positive, geo-mean 1.52x, 125 trades. The live bot's 2.76x result is not just a post-2021 bull artifact.
- **Progress chart fix:** DONE (`8e650c73`) — chart now includes the real live-bot Turtle ATR-only line (~2.76x) separately from the dual-exit research diagnostic line.
- **T100 historical replay mode:** DONE — `examples/live_bot_historical_replay.rs` feeds cached aligned daily bars directly through `src/live/bot.rs::process_bar()` in dry-run mode. Replay processed 10,806 closed-bar events and recorded 286 closed trades, matching `live_bot_exact_equity.rs` trade count.
- **Exact-live source of truth:** rerun 2026-05-12 09:35 UTC — 2.76x / daily Sharpe 1.02 / MaxDD 22.3% / 286 trades / 1,801 days.

## Highest-Value Decision

**The bottleneck is now external: Binance testnet API key + secret.**

Per anti-spin rule: live/testnet credentials have been the blocker for 5+ weeks. One readiness session has now been spent productively (historical replay mode). Do **not** pretend more Turtle-family backtests are the bottleneck.

If credentials are still absent next session, the correct action is one of:
1. leave code untouched and report blocked pending credentials, or
2. do a small execution-readiness/documentation task only if it removes a real deployment risk.

No more nearby strategy comparisons unless a materially new mechanism is proposed.

---

## Project Tracks

### Track A — Trust the Lab

**Status: GREEN enough for deployment readiness.**

Completed:
- Exact-live daily account harness: `examples/live_bot_exact_equity.rs`
- Turtle-only pre-2021 held-out validation: `examples/t99_turtle_only_pre2021.rs`
- Production event-path replay: `examples/live_bot_historical_replay.rs`
- Progress chart separation of live bot vs research diagnostic
- Fee/maker-fill sensitivity: fee-adjusted Sharpe range [1.02–1.04]

Regression guardrail:
- After any `src/live/bot.rs` or `src/live/config.rs` change, run:
  1. `cargo build`
  2. `cargo run --example live_bot_historical_replay --profile sweep 2>&1`
  3. `cargo run --example live_bot_exact_equity --profile sweep 2>&1`
- Expected current baseline: historical replay 286 closed trades; exact-live 2.76x / Sharpe 1.02 / MaxDD 22.3% / 286 trades.

### Track B — Stress the Current Leader

**Status: no further Turtle-family stress tests unless mechanism changes.**

Known structural risks:
- 2026 YTD underperformance: -3.2%, Sharpe -1.17. This is real and unresolved.
- Top-10 concentration: ~91% of log return; convex tail fragility remains structural.
- Universe sensitivity: hold-out universe validation is mixed; do not claim clean cross-universe generalization.

Do not run more parameter sweeps for EP/HM/HAP/FC/ATR_RANK/VOL_LOOKBACK without new evidence. These fights are settled.

### Track C — Broaden Edge Discovery

Only pursue if credentials are still unavailable and the work is clearly not another Turtle parameter fight.

Acceptable future work:
- Top-winner regime decomposition: explain the convex-tail mechanism behind SOL/DOGE/XRP winners.
- Materially new features/mechanisms with explicit convex-tail preservation guardrail.
- Execution readiness docs/runbooks tied directly to going live.

---

## Honest Production Statement

- **Live bot:** Turtle ATR-only exit, FIFO/equal-slot entries, no dollar-volume rank gate.
- **Current exact-live metrics:** 2.76x / Sharpe 1.02 / MaxDD 22.3% / 286 trades.
- **Pre-2021 validation:** CONFIRMED positive across 5/5 symbols.
- **Research harness 621x:** dual Chandelier+Turtle exit; research diagnostic only, not production performance.
- **Historical replay:** production `process_bar()` path now replayable without API keys; use as regression guard, not performance source of truth.
- **Main blocker:** Binance testnet credentials.

---

## Anti-Spin Rules (Active)

1. No Turtle-family parameter sweeps unless a new mechanism is proposed.
2. No more docs-only critique cycles that restate already closed gaps.
3. No conflating research harness equity with production live-bot equity.
4. No production claim without exact-live and historical-replay regression checks after code changes.
5. If credentials remain absent, say blocked plainly; do not invent benchmark fights.
6. Top-10 convex tail must be preserved by any future filter candidate.
7. 2026 YTD underperformance is a real structural risk, not a solved issue.
