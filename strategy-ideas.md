# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-06 20:05 UTC. Added C16/C17/C18. T61-ALT CLOSED (REJECTED). T80 still unbuilt.*

---

## Critical Alerts

### T61-ALT: CLOSED — REJECTED (2026-05-06)
Taker-buy pressure overlay candidate built and tested on exact live bot path:
- Exact live: 2.78x / Sharpe 1.00 / MaxDD 28.2% / 298 trades
- Pressure candidate: 2.76x / Sharpe 1.01 / MaxDD 25.3% / 275 trades
- Equity did not improve. Only 6/10 T73 top winners preserved. REJECTED permanently.
- Feature cache at `data/cache/taker_buy/` remains useful for Track C (feature infrastructure only, not entry filter).

### T80: OOS Universe Validation — UNBUILT (critical)
Reserve UNIUSDT, MATICUSDT, AVAXUSDT as explicit hold-out. 3 pairs × 6 windows. This is the most important test we could run — it validates whether our parameters generalize beyond the 9-pair learned grid. Listed as priority last session, not built. Must execute.

---

## Top Execution Tasks (Priority Order)

### T80: OOS Universe Validation — BUILD, NOT DOCUMENT
Status: Unbuilt for 2 sessions. Highest priority.
- Reserve UNIUSDT, MATICUSDT, AVAXUSDT as explicit hold-out (never optimized on these pairs).
- Run exact-live Turtle-only walk-forward: 3 pairs × 6 windows.
- Decision: pass rate ≥ 70% AND Sharpe ≥ 0.5 → new trust evidence. Pass rate < 60% → generalization boundary documented.

### T53-RESOLUTION: State the Blocker or Reduce Scope
Status: "Blocked" 5+ weeks. Unacceptable to persist.
- Option A: Reduce mock to daily-bar. Wire `src/live/bot.rs` → mock → verify signals match T65 harness. Compile and get green test.
- Option B: "Mock bypass blocked on [X]. Resolution: [Y]." Send to Noah in Discord.
- Do NOT write "blocked — revisit next session." Execute or close.

### C18: Maker-Fill Rate Stress Test — BUILD
Critical risk quantification missing before testnet deployment.
- Maker fill: we have one crash window (FTX, 70.6%). Maker fill could be 40% in sustained bear.
- Build: maker_fill_rate ∈ {0.30, 0.35, 0.40, ..., 0.80} (10 steps) → apply as post-hoc fee adjustment to exact live equity.
- Output: table maker_fill_rate → equity_mult → fee_adj_sharpe.
- Rationale: we know 2.81x at assumed 70% maker fill. We don't know equity at 40%. Must quantify range before testnet.

---

## New Concepts (2026-05-06 Session)

### C16: Regime-Conditional Chandelier Multiplier
**Mechanism:** Use regime classification (SMA21 vs SMA200 — binary, fast-moving, ~2-4x per year) to condition Chandelier multiplier:
- High-trend regime → CHAND_MULT=2.0 (tighter exit, protect gains)
- Low-trend regime → CHAND_MULT=2.50 (looser exit, let winners run)

**Why this is different from the dead vol-contingent Chandelier:** Prior vol-contingent attempt failed because 21-bar realized vol rank barely crosses 0.75/0.25 thresholds — too slow-moving and continuous. Regime classification is binary and fast (regime changes 2-4x per year, not continuously). The mechanism is the same (condition exit multiplier on regime) but the trigger is mechanistically different.

**Test:** 3 configs × 9 universes × 6 windows. Must beat static CHAND_MULT=2.0 AND CHAND_MULT=2.30 on both pass rate and Sharpe. Decision: both improve → promote; either degrades → reject and close.

### C17: Consecutive-Bar Momentum Filter
**Mechanism:** Require 2 consecutive closes above Turtle entry level (not just 1) before entering. Filters false breakouts that immediately reverse (common in chop). Time-confirmation vs ATR-based filtering.

**Difference from ATR_ENTRY_MULT:** ATR_ENTRY_MULT filters based on volatility-adjusted proximity to entry level. Consecutive-bar filter is about time-confirmation — requiring the breakout to sustain, not just occur. ATR_ENTRY_MULT was definitively rejected (mult=0.0 optimal, any filter hurts). Consecutive-bar is a different mechanism.

**Test:** Turtle baseline vs Turtle+consecutive-bar (2 consecutive closes above entry level). 9 universes × 6 windows. Must beat baseline on both pass rate and Sharpe.

### C18: Maker-Fill Rate Stress Test
**Already described in Top Execution Tasks above.**

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
- [ ] **T53**: Mock exchange resolution — UNBUILT (escalate or reduce scope)
- [ ] **T80**: OOS universe validation — UNBUILT (critical)
- [ ] **C18**: Maker-fill rate stress test — UNBUILT (critical)
- [ ] **C16**: Regime-conditional Chandelier multiplier — UNBUILT
- [ ] **C17**: Consecutive-bar momentum filter — UNBUILT

---

## Resolved / Closed

- **T61 (aggTrades original):** DEAD — 1000-row cap impractical; superseded by taker-buy kline data.
- **T61-ALT:** REJECTED — taker-buy pressure overlay destroyed equity and top winners.
- **T70 (semantic gap):** CLOSED — gap is structural (research ≠ live bot path), not parameter problem.
- **T72 VOL_LOOKBACK live gate:** REJECTED — 1.01x vs 2.56x; killed 8/10 top winners.
- **T74 TURTLE_ATR_MULT:** M=2.00 confirmed; no more nearby sweeps.
- **T75 HEDGE_ATR_PERIOD:** P=38 promoted; +0.25x live equity improvement.
- **T69 semantic alignment:** REJECTED — 1.02x (worse than live bot).
- **T67 HEDGE_ATR_PCT:** 101 identical values — inert, dead code.
- **T67 HEDGE_SIZE_MULT:** 0.40 — risk dial, not alpha.
- **T76 taker-buy pressure feature:** cache built; signal mixed; not an entry filter.
- **Vol-contingent Chandelier (realized vol rank):** DEAD — too slow-moving, all configs identical.
- **ATR_ENTRY_MULT>0:** DEAD — mult=0.00 definitively optimal.
- **EP=24:** Reverted — held-out failure.
- **Weekend filter:** Rejected — weekend entries are valuable.
- **ATR_RANK=24/65:** Failed held-out; T=5.0 is a risk dial, not alpha.

---

## Anti-Overfitting Rules

1. No Turtle-family parameter sweeps unless a new mechanism is proposed.
2. No "audit" tasks — write the test or close the issue.
3. New entry filter must pass top-winner preservation test before promotion.
4. Exact live-path daily equity required before quoting production metrics.
5. 176.79x research harness appears in HOF once: as diagnostic output, not production performance.
6. Every task has an execute-or-close decision — no deferred loops.
7. Per-window walk-forward Sharpe is not comparable to daily account Sharpe — never mix them.

---

## Sharpe Taxonomy (Required Labels)

| Type | Description | Honest Value |
|------|-------------|-------------|
| Daily compounded account | Real equity from exact-live replay | **1.01** |
| Per-window walk-forward | Mean of per-window Sharpe ratios | ~5.5 (inflated ~5x, not comparable) |
| Fee-adjusted (maker-fill range) | Realistic range at 30-80% maker fill | **0.6–1.3** (estimated) |

Report fee-adjusted Sharpe as a range, not a point estimate.