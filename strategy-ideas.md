# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-05 13:33 UTC. Critique cycle complete. T63/T64 are now complete. New top blind spot: source-of-truth drift between research harness, reports, and live bot. Current code uses AP=17, ATR_RANK=5, VL=92; exact live bot also has a hardcoded USDT hedge overlay that the latest research harnesses do not model.*

---

## Critical Alerts

### Source-of-Truth Drift Is Now the Main Risk

Current production evidence is split across incompatible files:
- `src/live/config.rs`: AP=17, T=5, VL=92.
- `src/live/bot.rs`: Turtle-only + ATR_RANK + hardcoded USDT hedge (`BTC ATR21 > 75th pct → size *= 0.70`) plus stale header comments still referencing AP=12.
- `examples/turtle_only_equity.rs` / T63: AP17/T5/VL92, but **no live-bot hedge overlay**.
- `reports/daily_progress.csv`: stale 33.9x / Sharpe 0.42 live-path number.
- `HALL_OF_FAME.md`: stale AP=12 and old dual-exit/live-path metrics.

**Action:** Build T65 exact live-bot source-of-truth harness before adding more alpha.

### USDT Hedge 3D Sweep Artifacts Are Untrusted

Pre-existing untracked files `examples/usdt_hedge_3d_sweep.rs` and `snapshots/usdt_hedge_*` exist, but the summary reports impossible pass counts (e.g. 68/63, 107.9%). Treat these as broken until the aggregation denominator/pass counting bug is fixed. Do not promote hedge params from them.

### ATR_RANK=5 Is Mixed, Not Clearly Defensive

T64 attribution:
- T=5: 114.19x / Sharpe 1.68 / MaxDD 51.2% / 154 trades.
- T=0: 206.95x / Sharpe 1.75 / MaxDD 45.9% / 189 trades.
- T=5 improves bear Sharpe slightly and trend-vol Sharpe materially, but cuts equity/trades and worsens attribution MaxDD.

**Decision:** Keep T=5 only because high thresholds failed held-out and T=0 has not been revalidated under exact live bot. Do not blindly trust the gate.

---

## Resolved

- **AP=63 ANTI-OVERFIT VIOLATION: RESOLVED ✔** (2026-05-04). AP=17 promoted to production after held-out validation.
- **T59 Turtle-Only Equity: DONE ✔** (updated 2026-05-05). Latest AP17/T5/VL92 research path = 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades, excluding live-bot hedge overlay.
- **T60 Per-Year Decomposition: DONE ✔**. 2022 remains dominant; exact-live year table still needs T65.
- **T63 Per-Trade Attribution: DONE ✔**. Not top-5-only; top 5 = 38.2% of log-return, top 10 = 60.9%.
- **T64 Regime Sharpe Decomposition: DONE ✔**. Direction regimes balanced; volatility regimes are weaker; ATR_RANK=5 is mixed.
- **VOL_LOOKBACK drift:** config.rs/live-compatible/Turtle-only research path now use VL=92 ✅
- **Short-side sleeve:** GRAVEYARD'd (37.5% pass vs 69.1% guardrail) ✅
- **SIZE_MULT:** INERT — cosmetic risk knob only ✅

---

## Top 3 Most Promising Unbuilt Ideas

### T65: Exact Live-Bot Source-of-Truth Harness (IMMEDIATE)
**Status:** NEW / UNBUILT.
- **Problem:** Current best metrics validate a research approximation, not necessarily the exact `src/live/bot.rs` path. The live bot includes a hardcoded USDT hedge overlay not modeled in T63/T64.
- **Action:** Build one canonical harness matching `src/live/bot.rs`: Turtle entry, AP17/T5 ATR_RANK gate, VL92 volume ranking, Turtle ATR exit, cap=3, fees, and USDT hedge sizing.
- **Why:** This is more important than new alpha. Without it, HOF/reports/Discord can keep quoting wrong numbers.
- **Output:** `snapshots/live_bot_exact_equity.md` + CSV, including daily equity, Sharpe, MaxDD, trades, yearly table, and trade concentration.

### T62: Weekend Effect Filter (HIGH — 1 session)
**Status:** UNBUILT.
- **Problem:** Weekend crypto liquidity is thinner; weekend breakout bars may be more prone to false moves and slippage.
- **Action:** On the exact T65 harness, compare baseline vs skip/reduce-size Saturday/Sunday entries. Audit whether top-10 winners are skipped.
- **Why:** Simple, mechanistically plausible, no new data, directly execution-relevant.
- **Acceptance rule:** Promote only if pass rate/DD improve without deleting the rare breakout winners that drive convex return.

### T61: Binance aggTrades Order Flow Signal (HIGH — 1-2 sessions after T65)
**Status:** UNBUILT.
- **Problem:** LOB NOBI is still data-blocked. Need microstructure alpha that can use public historical data.
- **Action:** Download historical Binance `aggTrades`; aggregate buyer/seller-initiated imbalance into 5-min/daily flow features; test as Turtle entry confirmation or size scalar.
- **Why:** Genuinely new information source, not another price-only parameter sweep.
- **Risk:** Must avoid over-filtering rare breakout winners; use T63 top-trade skip audit as a guardrail.

---

## Infrastructure / Trust (Must Fix)

- [ ] **T65**: Exact live-bot source-of-truth harness — highest priority.
- [ ] **T66**: Regenerate HOF/reports from T65; label daily vs WF vs attribution Sharpe.
- [ ] **T53**: Mock exchange — still the practical bypass for missing Binance testnet keys.
- [ ] **T55**: LOB NOBI data collection — still missing; do not claim it is one harness away.

### Bypass Blocker: Local Mock Exchange
**Status:** UNBUILT. 5+ weeks overdue.
- Noah's API keys blocking testnet.
- Build Rust HTTP/WS server that mocks binance-rs-async endpoints.
- Seed with historical 1m parquet data from `data/cache/`.
- Simulate fills, slippage, order state, account balance, and reconnect behavior.
- Test `src/live/bot.rs` end-to-end without Binance credentials.

---

## New Concepts Added From Critique

### C1: Top-Trade Skip Audit
Every new entry filter must report whether it skipped any of the historical top-10 / top-20 winning trades from T63. A filter that improves average Sharpe by avoiding small losers but misses rare convex winners is probably fake safety.

### C2: MaxDD Tolerance / Abandonment Stress Test
The strategy's 99.5% MaxDD is not psychologically or operationally survivable. Add an "abandonment" metric: what is performance if capital is cut, trading halted, or risk reduced after 50%, 70%, 85%, and 95% drawdowns? A strategy that only works if the operator survives a 99% drawdown is not deployable as-is.

### C3: Sharpe Taxonomy Rule
Every report must label Sharpe as one of:
- daily compounded account Sharpe,
- per-window walk-forward average Sharpe,
- attribution Sharpe,
- milestone-aggregated Sharpe.
Never compare these as if they are the same metric.

---

## Tested and Rejected (Do Not Revisit Without New Mechanism)

| Strategy | Result | Key Reason |
|----------|--------|------------|
| ATR_RANK=24 | **GRAVEYARD** | Held-out: 10/22 pass/-0.964 Sharpe vs T=5 14/22/+0.664. Same-harness artifact. |
| ATR_ENTRY_MULT (all) | REJECTED | EM=0.00 wins definitively |
| Short-side sleeve | **GRAVEYARD** | 37.5% pass vs 69.1% guardrail. Signal too sparse. |
| SIZE_MULT overlay | INERT | Return/DD scale linearly with M — pure risk knob. |
| EP=24 | REVERTED | Held-out: 25/29 vs EP=21 27/29. Same-harness artifact. |
| Funding Rate Regime Filter | NULL/REJECTED | Pass rate never improves across 4,590 runs. |
| Donchian sleeve | REJECTED | 63% < 69.1% guardrail despite Sharpe lift. |
| Mid-caps | REJECTED | 60% < 70% threshold. |
| 4h Multi-Timeframe Turtle | GRAVEYARD | 1/20 pass — structural timeframe incompatibility. |
| BollingerReversion | GRAVEYARD | 0/288 OOS — signal actively harmful. |
| BTC-ETH cointegration | GRAVEYARD | All 12 configs negative Sharpe, -16 to -46% return. |
| ATR-norm position sizing | REJECTED | Equal capital optimal; ATR-norm inverts vol ranking. |
| A/D Dual-Hat static sleeve | REJECTED | Below-random win rate. |
| CTREND fixed 25% sleeve | REJECTED | Sharpe destroyed 1.38→0.33. |

---

## Anti-Overfitting Rules

1. Minimum 3-window (5.5%) improvement on OOS before accepting any param change.
2. No sequential optimization on same data; held-out validation required for marginal wins.
3. Never promote a same-family parameter tweak without checking whether it sits on a broad plateau.
4. Equity curve dominance required (>80% of time bars).
5. Absolute guardrails over relative improvement.
6. New filters must pass a top-trade skip audit.
7. Equity numbers must match the exact live path before being quoted as production.
8. All reports must label Sharpe methodology.

---

## Key Insight: All Metrics Are Upper Bounds Until Exact Live Validation

The edge is probably real. The production readiness is not. The next breakthrough is not another threshold sweep; it is making the live bot, research harness, HOF, reports, and Discord updates all speak the same truth.
