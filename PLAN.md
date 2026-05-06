# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-06 16:06 UTC — Critique session: documented loop, execution tasks defined**

## Current Truth

- **Exact as-coded live bot:** 2.81x / daily account Sharpe 1.01 / MaxDD 28.2% / 298 trades / 1,795 days. This is the production source of truth. Everything else is diagnostic.
- **Research harness (diagnostic only):** 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades — different system, different risk, not comparable to live bot path. Do not cite as production performance.
- **No more Turtle-family parameter sweeps.** Last 8 commits: all same-family Turtle tuning. ATR_MULT confirmed, ATR_ENTRY reverted, HEDGE_ATR_PCT inert, HEDGE_SIZE is a risk dial, Freshness→0, HEDGE_ATR_PERIOD→38. The parameter space is exhausted. Next Turtle sweep = wasted cycle.

## Critical: We Are In A Documentation Loop

T70 (semantic gap audit) has appeared in PLAN.md as "next priority" for 4+ weeks without being executed. T61 (aggTrades order-flow) has been listed as "highest priority after T70" for 6+ weeks. T53 (mock exchange) has been "blocked" for 5+ weeks without a clear resolution statement.

Every session produces a critique commit updating PLAN.md, and the next session begins by reading those same documents. We are optimizing documentation velocity, not system quality.

**Rule for this session:** do not add another "audit" or "feasibility" task. Execute or close.

## Biggest Blind Spots

1. **No genuine out-of-sample universe.** Same 9-universe grid used since April — effectively "learned" through repetition. Reserve 2-3 non-standard pairs (UNI, MATIC, AVAX) as explicit hold-out never-optimized validation.
2. **Maker-fill assumption has one crash data point.** FTX window shows 70.6% — directionally positive but not proven across multiple crashes. Fee drag could be 22% or 50%. Report fee-adjusted Sharpe as a range.
3. **We find bugs through documentation, not tests.** T34 Chandelier gap, T67 inert param, T72 VL gap — all discovered by reading code/docs, not by automated detection. Write the test before documenting the gap.
4. **Top-trade concentration = 82.8% of log return.** One filter mistake can destroy 0.5x of equity. Guardrail exists (T73 skip audit) but is only applied once.

## Next Tasks (Priority Order)

### T61-ALT: Taker-Buy Pressure Overlay Candidate — EXECUTE (not document)
**Status:** T76 built the feature cache and confirmed mixed signal. This is the unfinished part.
- Do NOT just document "mixed signal" — build the Turtle entry + taker-buy pressure overlay candidate that specifically preserves T73 top-10 winners.
- Use `data/cache/taker_buy/` parquet files (already downloaded).
- Gate: long only if pressure > 50th pct AND symbol not in T73 skip-list.
- Benchmark against exact-live without pressure overlay.
- If candidate improves Sharpe AND preserves top-10 winners → promote.
- If candidate degrades Sharpe OR kills top winners → reject and close T61 permanently.

### T53-RESOLUTION: State The Blocker Or Reduce Scope
**Status:** "Blocked" for 5+ weeks without resolution.
- Option A: Reduce mock scope to daily-bar. Run end-to-end decision-path test using existing daily parquet. Wire `src/live/bot.rs` → mock → verify signals match T65 harness output.
- Option B: Write a one-paragraph blocker statement: "Mock bypass blocked on [1m parquet OR testnet API keys]. Resolution: [X]." Send to Noah in Discord.
- Do NOT leave T53 as "blocked — will revisit after T70." Either reduce scope or escalate.

### T80: Out-Of-Sample Universe Validation
**Status:** NEW.
- Reserve UNIUSDT, MATICUSDT, AVAXUSDT as explicit hold-out universes.
- Run exact-live Turtle-only walk-forward on these 3 pairs (6 windows each).
- Decision: if pass rate ≥ 70% AND Sharpe ≥ 0.5 → no changes needed but we have new validation evidence. If pass rate < 60% → document that strategy generalizes to our optimized universe but not to held-out pairs.

## Resolved / Closed

- T72 VOL_LOOKBACK live gate: rejected (1.01x vs 2.56x)
- T74 TURTLE_ATR_MULT: 2.00 confirmed (no more nearby sweeps)
- T75 HEDGE_ATR_PERIOD: 38 promoted (real improvement)
- T69 semantic alignment: rejected (worse than live bot)
- T67 HEDGE_ATR_PCT: inert, all 101 values identical
- T76 taker-buy pressure: feature cache built, signal mixed, candidate not yet built
- T73 top-winner audit: guardrail documented (do not add filters that kill convex winners)

## Resolved Concepts (Do Not Revisit)

- VOL_LOOKBACK live top-3 gate: rejected by T72
- TURTLE_ATR_MULT: 2.00 reconfirmed; no more nearby ATR_MULT sweeps
- HEDGE_ATR_PCT: 101 identical — dead code
- ATR_ENTRY_MULT: 0.00 definitively optimal
- FRESHNESS_COOLDOWN: 0 wins (live bot updated)
- EP=24: reverted (held-out failure)
- Weekend filter: rejected
- ATR_RANK=24/65: failed held-out
- Donchian: 63% < 69.1% guardrail
- All non-trend strategies: dead or borderline
- Raw aggTrades (T61 original): impractical (1000-row cap); superseded by taker-buy kline data

## Parameters (Frozen)

```text
TURTLE_EP=21, TURTLE_ATR_PERIOD=24, TURTLE_ATR_MULT=2.00, ATR_ENTRY_MULT=0.00,
HOLD_MAX=12, POSITION_CAP=3, FRESHNESS_COOLDOWN=0,
REGIME_ATR_PERIOD=17, REGIME_LOOKBACK=41, ATR_RANK_THRESHOLD=5.0,
VOL_LOOKBACK=92 (diagnostic-only; live rank gate rejected by T72),
HEDGE_ATR_PERIOD=38, HEDGE_LOOKBACK=252, HEDGE_ATR_PCT=0.45, HEDGE_SIZE_MULT=0.40,
fee_pct=0.000400
```

## Anti-Spin Rules

1. No more Turtle-family parameter sweeps unless a new mechanism is proposed.
2. No more "audit" tasks — write the test or close the issue.
3. Every new task must have an execute-or-close decision, not a "defer to next session" path.
4. The 176.79x number appears in HOF once only: as diagnostic output, not production performance.