# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-07 00:12 UTC — CRITIQUE SESSION. C17/C18/C16 remain unbuilt (2 sessions overdue). T80 OOS result: 11/18 pass (61.1%), avg Sharpe 0.149 — borderline, not clean.**

## Current Truth

- **Exact as-coded live bot:** 2.81x / daily account Sharpe 1.01 / MaxDD 28.2% / 298 trades / 1,795 days. Production source of truth. Everything else is diagnostic.
- **Research harness (diagnostic only):** 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades — different system, not comparable. Do not cite as production performance.
- **Per-window walk-forward Sharpe (~5.5):** INFLATED ~5x vs daily account Sharpe. Not comparable. Never cite on equity charts. Only 1.01 (daily account) is honest.
- **No more Turtle-family parameter sweeps.** Last 8+ commits: all same-family Turtle tuning. ATR_MULT confirmed, ATR_ENTRY reverted, HEDGE_ATR_PCT inert, HEDGE_SIZE is a risk dial, Freshness→0, HEDGE_ATR_PERIOD→38. Parameter space is exhausted.
- **T78 (ATR_RANK T=5.0) was another documentation loop.** The parameter is non-stationary (held-out fails at T=24 and T=65). T=5.0 "won" in the 9-universe grid — but winning in a non-stationary space is noise, not signal. ATR_RANK_THRESHOLD is a risk dial (T=5.0 > T=0), not an alpha source.
- **T80 OOS hold-out validation is mixed, not a clean pass.** Exact-live Turtle-only on UNI/MATIC/AVAX hold-outs: **11/18 pass (61.1%), avg Sharpe 0.149, avg return +2.3%/window, 160 trades**. MATIC passed 6/6, AVAX 4/6, UNI 1/6. This fails the PLAN promotion guardrail (≥70% pass and Sharpe ≥0.5), but is not a catastrophic <60% failure.

## Critical: Documentation Loop Pattern — Still Active

T70 (4+ weeks), T61 (6+ weeks), and T53 (5+ weeks) show the loop pattern. T80 was finally executed this session after being listed but not built; T53 must not get another defer. Every documentation-only cycle compounds the problem.

**Research is CLOSED.** Only API keys (Noah's Binance testnet keys) remain as the blocker. All remaining work is monitoring infrastructure or closing remaining ideas.

## C16: CLOSED PERMANENTLY — Mechanistically Invalidated (2026-05-07)

CHAND_PERIOD 98-value extensive sweep (e6f4ed05) proved all 98 values produce IDENTICAL results: Sharpe=2.797, pass=68.3%, equity=1.4538x. Root cause: Turtle ATR exit fires before Chandelier in dual-exit architecture. Chandelier is non-binding in most trades.

C16 (regime-conditional Chandelier multiplier) attempted to modulate CHAND_MULT by regime. But modulating the multiplier of a non-binding exit has zero effect on outcomes that are already decided by Turtle ATR. **Close permanently. Remove from strategy-ideas.md.**

## Biggest Blind Spots (Honest Assessment)

1. **We only test in a "learned" universe.** T80 partially closed this by testing UNI/MATIC/AVAX outside the repeated 9-universe grid. Result was **mixed/borderline** (61.1% pass, Sharpe 0.149), so the production edge is still universe-sensitive; do not claim clean cross-universe generalization.

2. **Equity is dangerously concentrated.** Top 10 log contributors = 82.8% of compounded return. No automated detection — only discovered because we manually audited T73. One bad filter silently destroys the tail.

3. **We test strategies on the walk-forward harness, not on the exact live bot path.** T72 and T69 both passed harness validation but failed exact-live bot semantics. We have no integration gate that runs candidates through `src/live/bot.rs` entry logic before citing results.

4. **2021/2022 chop regimes are underweighted in pass-rate calculations.** Full-history pass rates are inflated by mega-bull windows (2018, 2020, 2023, 2024). The worst-case regime (2021 chop, +35.5%) is underweighted.

5. **Fee-adjusted Sharpe is reported as a point estimate, not a range.** We have one crash window (FTX, 70.6% maker fill). Maker fill could degrade to 40% in sustained bear. Equity at 50% maker fill is unknown.

## Next Tasks (Priority Order) — Execute, Not Document

### C18: Maker-Fill Rate Stress Test — ACCEPTABLE ✅
**Status:** Completed (2026-05-07, 03:25 UTC). Post-hoc fee sensitivity on exact-live equity.
- Equity range: 2.80x–2.85x across maker_fill ∈ [30%, 100%] — very low sensitivity
- Fee-adjusted Sharpe: ~0.839–0.852
- At 40% maker fill: equity=2.808x, Sharpe=0.841, MaxDD=28.1%
- **Decision:** C18 ACCEPTABLE. Equity above 1.5x threshold at 40% maker fill. No Arc escalation needed.
- C16 may proceed only if live-path relevant. `src/live/bot.rs` is Turtle ATR-only; Chandelier logic is diagnostic/research-harness unless explicitly reintroduced.

### M1: Equity Trajectory Monitor — BUILD
**Status:** Priority. No automated monitoring exists for strategy degradation.
- Compute 60-day rolling return of exact-live equity
- Alert if rolling return falls below calibrated threshold (based on W04/W05 historical range)
- This is operational infrastructure for testnet deployment, not research.

### M2: Maker-Fill Rate Tracker (Definitive) — BUILD
**Status:** Priority. C18 was applied to T65 harness path, not the current exact-live path with T75 hedge parameters.
- Replay `snapshots/live_bot_exact_trades.csv` with maker_fill ∈ {0.30, 0.35, ..., 1.00}
- Compute fee-adjusted equity per step using correct live-style fee model
- Document equity range for definitive testnet risk assessment.
- No new data download — uses existing exact-live trade CSV.

## Resolved / Closed

- **C17 (consecutive-bar filter):** CLOSED PERMANENTLY — 81% vs 93.7% pass, -2.9% equity delta, fails dual-gate
- **C16 (regime-conditional Chandelier):** CLOSED PERMANENTLY — CHAND_PERIOD 98-value sweep proved Chandelier non-binding; modulating multiplier of non-binding exit has zero effect
- **CHAND_PERIOD inertness:** PROVED — 98-value sweep (e6f4ed05) confirmed all values identical; prior "CP=7 winner" was sparse-grid artifact
- T72 VOL_LOOKBACK live gate: rejected (1.01x vs 2.56x)
- T74 TURTLE_ATR_MULT: 2.00 confirmed (no more nearby sweeps)
- T75 HEDGE_ATR_PERIOD: 38 promoted (real improvement)
- T69 semantic alignment: rejected (worse than live bot)
- T67 HEDGE_ATR_PCT: inert, all 101 values identical
- T61/T76 taker-buy pressure: REJECTED/CLOSED. Feature cache exists; candidate rejected (equity no better, 6/10 top winners).
- T73 top-winner audit: guardrail documented (preserve top winners before any filter promotion)
- T78 ATR_RANK T=5.0: confirmed but mechanism is non-stationary; threshold is risk dial not alpha
- T80 OOS hold-out universe validation: MIXED/BORDERLINE — UNI/MATIC/AVAX exact-live Turtle-only 11/18 pass (61.1%), avg Sharpe 0.149; MATIC strong, AVAX borderline, UNI weak. Do not cite as clean generalization.
- T53 mock exchange: CLOSED (Option A — signal path verified via code review + exact-live harness; LiveBot::bars private; MockExchange cost smoke passed; only real blocker is API keys)

## Resolved Concepts (Do Not Revisit)

- VOL_LOOKBACK live top-3 gate: rejected by T72
- TURTLE_ATR_MULT: 2.00 reconfirmed; no more nearby ATR_MULT sweeps
- HEDGE_ATR_PCT: 101 identical — dead code
- ATR_ENTRY_MULT: 0.00 definitively optimal
- FRESHNESS_COOLDOWN: 0 wins (live bot updated)
- EP=24: reverted (held-out failure)
- Weekend filter: rejected
- ATR_RANK=24/65: failed held-out; T=5.0 is risk dial only
- Donchian: 63% < 69.1% guardrail; not Turtle replacement
- All non-trend strategies: dead or borderline
- Vol-contingent Chandelier (realized vol rank): dead (too slow-moving)
- Raw aggTrades (T61 original): impractical; superseded by taker-buy kline data
- Taker-buy pressure entry overlay: rejected (equity no better, top winners not preserved)

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
3. Every task must have an execute-or-close decision. No "defer to next session."
4. The 176.79x number appears in HOF once: as diagnostic output, not production performance.
5. Daily account Sharpe (~1.01) only on equity charts. Per-window walk-forward Sharpe is not comparable.
6. Report fee-adjusted Sharpe as a range (maker fill uncertain), not a point estimate.