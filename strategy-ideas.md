# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-06 12:14 UTC. Critical findings: (1) production live bot is 2.81x / daily account Sharpe 1.01, not Sharpe 5+; (2) T72/T73/T74/T75 closed the obvious trust gaps; (3) remaining high-value work is execution, order-flow, and untouched OOS stress, not another Turtle parameter sweep.*

---

## Critical Alerts

### Production Sharpe Is 1.01, Not 5+
The only production-facing metric is the exact live bot after T75: **2.81x / daily account Sharpe 1.01 / MaxDD 28.2% / 298 trades / 1,795 days**. Walk-forward Sharpe 5+ figures are research/per-window diagnostics and must not be quoted as live account performance.

### Convex Tail Dependence Is The Central Strategy Risk
T73 proved top-10 exact-live trades account for **82.8%** of compounded log return. The largest winners are not obvious high-volume bull entries: entry regimes were **5 bear / 3 chop / 2 bull**, and a VL=92 top-3 gate would have excluded **8/10**. Any new filter must be treated as guilty until it proves it preserves the convex tail.

### The VOL_LOOKBACK Gap Is Resolved, Not A Fix
`src/live/bot.rs` does not use VOL_LOOKBACK ranking. T72 isolated that change and found it harmful: **1.01x / Sharpe 0.09 / MaxDD 30.1%** vs exact live **2.56x / Sharpe 0.95** before T75. Do not keep reopening this as a missing one-line production patch.

### The Remaining Blind Spot Is Generalization + Execution
We have repeatedly optimized on the same 9-universe grid. We also still lack an end-to-end mock/live execution path that drives the real `LiveBot::process_bar` through orders/fills/state. The research story is much stronger than the deployment story.

---

## Top 3 Most Promising Unbuilt Ideas

### T53: Daily-Bar Mock Exchange Bypass — DEPLOYABILITY FIRST
**Status:** UNBUILT / cost smoke only.
- Scope-reduce from perfect 1m HTTP/WS mock to daily-bar mock replay first.
- Wire `src/live/bot.rs` / `LiveBot::process_bar` through a mock exchange adapter.
- Reconcile generated orders/fills/state against exact-live trade ledger.
- Why: without this, we have a backtest and a bot, but not proof that the actual live decision path behaves as researched.

### T61: Binance aggTrades Order-Flow Signal — TRUE NEW INFORMATION
**Status:** UNBUILT.
- Download historical Binance `aggTrades`; aggregate taker buy/sell imbalance, large-trade imbalance, trade-count imbalance, and persistence into daily features.
- Start with sizing/confirmation, not hard filtering, because top-winner deletion risk is high.
- Why: every recent improvement is price-only. Order flow is the only credible new information dimension in the queue.

### T76: Untouched OOS Universe / Era Stress Test — CURVE-FIT FALSIFICATION
**Status:** UNBUILT.
- Build a symbol/era validation set not used in April/May parameter selection.
- Include legacy/lower-liquidity Binance survivors and earlier eras where possible.
- Freeze current production params before testing.
- Why: the 9-universe grid has become part of the optimization process. We need a fresh falsification set.

---

## Recently Closed / Updated

- **T75 HEDGE_ATR_PERIOD:** promoted 21 -> 38. Winner 50/60 pass, Sharpe 1.432, avg return +22.12%, DD 11.51%. Exact-live rerun now 2.81x / Sharpe 1.01 / MaxDD 28.2%.
- **T73 top-winner conditions audit:** done. Top-10 = 82.8% of log return; VL top-3 kills 8/10; high ATR_RANK kills 5-8/10; convex winners come from ugly regimes.
- **T72 VOL_LOOKBACK live gate:** rejected. Adding VL ranking to exact live semantics collapses performance to 1.01x.
- **T74 TURTLE_ATR_MULT:** M=2.00 reconfirmed. No config change.
- **T69 semantic patch:** rejected. Patching toward research harness made live replay worse.
- **T67/HOF cleanup:** production metrics now come from exact-live path only.

---

## New Concepts Added From 2026-05-06 12:14 Critique

### C15: Production-Sharpe Reality Rule
If a metric is not daily compounded account equity from exact live semantics, it is not a production Sharpe. Research Sharpe 5+ can guide exploration but cannot be used as a deployment claim.

### C16: Convex-Tail Preservation Rule
Any filter, ranking gate, sizing rule, or order-flow confirmation must report how many top-10 and top-20 exact-live log contributors it would have changed or removed. Improvement that deletes convex winners is fake safety.

### C17: Untouched OOS Grid Rule
Walk-forward validation on the same repeatedly used 9-universe grid is no longer enough for promotion. Same-family changes require a fresh symbol/era stress set that was not used during parameter discovery.

### C18: Daily Mock First Rule
When full microstructure replay is blocked by missing 1m data, build the daily-bar live-path mock first. A reduced-scope `LiveBot::process_bar` reconciliation is more valuable than another document saying the full mock is blocked.

---

## Tested and Rejected (Do Not Revisit Without New Mechanism)

| Strategy | Result | Key Reason |
|----------|--------|------------|
| T69 semantic alignment patch | REJECTED | Made live equity WORSE (1.02x vs 2.54x). Gap is structural. |
| HEDGE_ATR_PCT (101 values) | NULL | All identical — inert parameter |
| Weekend Entry Filter | REJECTED | 58/63 → 56/63 pass; weekend entries are valuable |
| ATR_RANK=24 | GRAVEYARD | Held-out: 10/22 pass/-0.964 Sharpe vs T=5 14/22/+0.664 |
| ATR_ENTRY_MULT (all) | REJECTED | EM=0.00 wins definitively |
| Short-side sleeve | GRAVEYARD | 37.5% pass vs 69.1% guardrail |
| EP=24 | REVERTED | Held-out failed vs EP=21 |
| Funding Rate Regime Filter | REJECTED | Pass rate never improves across 4,590 runs |
| Donchian sleeve | REJECTED | 63% < 69.1% guardrail |
| BollingerReversion | GRAVEYARD | 0/288 OOS; signal actively harmful |
| BTC-ETH cointegration | GRAVEYARD | All configs negative Sharpe |
| CTREND fixed 25% sleeve | REJECTED | Sharpe destroyed 1.38→0.33 |
| Vol-Contingent Chandelier (21-bar) | GRAVEYARD | All configs identical |
| Vol-Contingent Chandelier (252-bar) | GRAVEYARD | Tied (+0.12 noise) — no adaptive benefit |

---

## Anti-Overfitting Rules

1. Minimum 3-window / 5.5pp improvement before accepting same-family param changes.
2. Held-out validation required for marginal wins.
3. Plateau + era robustness required before promotion.
4. **Top-trade skip audit required for every filter.**
5. **Exact live-path daily equity required before quoting production metrics.**
6. Methodology labels required for every Sharpe.
7. No more nearby Turtle filters until T53/T61/T76 produce genuinely new evidence.

---

## Current Key Insight

The edge is probably real but modest: exact-live production is **2.81x / daily account Sharpe 1.01 / MaxDD 28.2%**, not a Sharpe-5 money printer. T72/T73 resolved the dangerous VOL_LOOKBACK and convex-tail questions. The next move is execution realism (T53), genuinely new data (T61), and untouched OOS falsification (T76).
