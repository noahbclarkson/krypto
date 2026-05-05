# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-05 16:05 UTC. Critique cycle complete. Core Turtle edge is probably real, but production truth is still fragmented. Current code uses AP=17, ATR_RANK=5, VL=92, HEDGE_ATR_PCT=0.45, HEDGE_SIZE_MULT=0.40.*

---

## Critical Alerts

### Source-of-Truth Drift Is Still the Main Risk

Current production evidence is split across incompatible artifacts:
- `src/live/config.rs`: AP=17, T=5, VL=92, hedge pct=0.45, hedge size=0.40.
- `src/live/bot.rs`: exact live decision path, but not yet the canonical daily-equity source.
- `examples/turtle_only_equity.rs` / T63: AP17/T5/VL92 daily equity, but no current hedge overlay.
- `examples/live_compatible_wf.rs`: includes live-compatible hedge behavior, but reports per-window WF Sharpe, not exact daily account Sharpe.
- `reports/daily_progress.csv`: contains stale/misleading `ATR_RANK=24` live-bot row.
- `HALL_OF_FAME.md`: stale AP=12 / dual-exit production evidence.

**Action:** Build T65 exact live-bot source-of-truth harness before any new alpha work.

### Sharpe 5+ Is Mostly Methodology, Not Magic

Walk-forward average Sharpe values around 6–7 are useful diagnostics, but not investor-real account Sharpe. Every report must state whether Sharpe is daily compounded account Sharpe, per-window WF Sharpe, attribution Sharpe, or milestone-aggregated Sharpe.

### MaxDD Is the Uncomfortable Truth

Turtle-only research equity shows 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades. The edge may be real, but a strategy that requires surviving near-total drawdown is not production-safe without an abandonment/risk-reduction analysis.

---

## Top 3 Most Promising Unbuilt Ideas

### T65: Exact Live-Bot Source-of-Truth Harness (IMMEDIATE)
**Status:** UNBUILT.
- **Problem:** No single artifact answers what `src/live/bot.rs`, as currently coded, would have done historically.
- **Action:** Build one canonical harness matching live bot behavior exactly: Turtle entry, AP17/LB42/T5 ATR_RANK, VL92 ranking, Turtle ATR exit, cap=3, fees, hedge pct=0.45, hedge size=0.40.
- **Output:** `snapshots/live_bot_exact_equity.md` + CSV with daily equity, daily Sharpe, MaxDD, trades, yearly table, top-trade concentration, and parameter/methodology table.
- **Why:** More important than new alpha. Without this, HOF/reports/Discord can keep quoting incompatible production numbers.

### T68: MaxDD Abandonment / Risk-of-Ruin Stress Test (HIGH)
**Status:** UNBUILT.
- **Problem:** 99.5% MaxDD is not survivable in real deployment psychology or risk governance.
- **Action:** On exact-live T65 output, simulate: stop trading after 50/70/85/95% DD, halve size after those thresholds, resume after new equity highs, and permanent capital impairment cases.
- **Output:** final equity, recovery time, missed top trades, and deployability verdict.
- **Why:** This tells us whether the strategy only works if no human ever turns it off during pain.

### T61: Binance aggTrades Order-Flow Signal (BEST TRUE ALPHA)
**Status:** UNBUILT, start after T65/T68.
- **Problem:** Most recent work is price-only parameter/risk tuning. Need new information.
- **Action:** Download public historical Binance `aggTrades`; aggregate taker buy/sell imbalance into daily confirmation or size-scalar features.
- **Guardrail:** Must pass top-trade skip audit. Reject if it filters out rare convex winners even when average Sharpe improves.
- **Why:** Genuinely new public microstructure signal without waiting for LOB NOBI infrastructure.

---

## Infrastructure / Trust Tasks

- [ ] **T65**: Exact live-bot source-of-truth harness.
- [ ] **T67**: Regenerate HOF/reports from T65 only; renumbered because T66 was used for hedge-size sweep.
- [ ] **T68**: Drawdown abandonment / risk-of-ruin stress.
- [ ] **T53**: Local mock exchange bypass for missing Binance testnet keys.
- [ ] **T55**: LOB NOBI collection/persistence; do not claim it is ready until data exists.

### T53: Local Mock Exchange
**Status:** UNBUILT. 5+ weeks overdue.
- Build Rust HTTP/WS server that mocks Binance endpoints used by `binance-rs-async`.
- Seed from historical 1m parquet.
- Simulate fills, slippage, order state, account balance, disconnect/reconnect.
- Test `src/live/bot.rs` end-to-end without Noah's Binance testnet credentials.

---

## Resolved / Closed

- **AP=63 anti-overfit violation:** resolved; AP=17 promoted after held-out validation.
- **T59 Turtle-only equity:** done; 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades, excluding hedge overlay.
- **T63 per-trade attribution:** done; top 5 = 38.2% of log-return, top 10 = 60.9%; not a single-trade mirage, but convex winners matter.
- **T64 regime decomposition:** done; bull/bear balanced; vol regimes weaker; ATR_RANK=5 mixed.
- **T62 weekend filter:** rejected; weekend entries improve, not hurt, the strategy.
- **T66 hedge-size sweep:** done; HEDGE_SIZE_MULT 0.70 → 0.40 as a risk dial.
- **VOL_LOOKBACK drift:** current code uses VL=92.

---

## New Concepts Added From Critique

### C1: Production Equivalence Test
Every production metric must trace to the same constants as `src/live/config.rs`. If a harness duplicates strategy logic, it must print the full param table and fail loudly if labels disagree with code.

### C2: Top-Trade Skip Audit
Every new entry filter must report whether it skipped historical top-10 / top-20 winning trades. A filter that avoids small losers but misses convex winners is fake safety.

### C3: MaxDD Abandonment Metric
Report what happens if the operator halts, halves size, or withdraws capital after 50%, 70%, 85%, and 95% drawdowns. A strategy that only works if nobody reacts to 99% DD is not deployable.

### C4: Sharpe Taxonomy Rule
Every report must label Sharpe as one of: daily compounded account, per-window walk-forward, attribution, or milestone-aggregated. Never compare them as if equivalent.

### C5: Same-Family Hyperopt Quarantine
Nearby tweaks to Turtle params, ATR_RANK, hedge pct, hedge size, or calendar filters require held-out/era stress and top-trade audit before promotion. One-window pass improvements are not enough.

---

## Tested and Rejected (Do Not Revisit Without New Mechanism)

| Strategy | Result | Key Reason |
|----------|--------|------------|
| Weekend Entry Filter (T62) | **REJECTED** | 58/63 → 56/63 pass; Sharpe 7.079 → 6.489; weekend entries are valuable. |
| ATR_RANK=24 | **GRAVEYARD** | Held-out: 10/22 pass/-0.964 Sharpe vs T=5 14/22/+0.664. |
| ATR_ENTRY_MULT (all) | REJECTED | EM=0.00 wins definitively. |
| Short-side sleeve | **GRAVEYARD** | 37.5% pass vs 69.1% guardrail. |
| SIZE_MULT overlay | INERT | Return/DD scale linearly; pure risk knob. |
| EP=24 | REVERTED | Held-out failed vs EP=21. |
| Funding Rate Regime Filter | NULL/REJECTED | Pass rate never improves across 4,590 runs. |
| Donchian sleeve | REJECTED | 63% < 69.1% guardrail despite Sharpe lift. |
| Mid-caps | REJECTED | 60% < 70% threshold. |
| 4h Multi-Timeframe Turtle | GRAVEYARD | 1/20 pass; timeframe incompatibility. |
| BollingerReversion | GRAVEYARD | 0/288 OOS; signal actively harmful. |
| BTC-ETH cointegration | GRAVEYARD | All configs negative Sharpe. |
| ATR-norm position sizing | REJECTED | Equal capital optimal; ATR-norm inverts vol ranking. |
| A/D Dual-Hat static sleeve | REJECTED | Below-random win rate. |
| CTREND fixed 25% sleeve | REJECTED | Sharpe destroyed 1.38→0.33. |

---

## Anti-Overfitting Rules

1. Minimum 3-window / 5.5pp improvement before accepting same-family param changes.
2. Held-out validation required for marginal wins.
3. Plateau + era robustness required before promotion.
4. Top-trade skip audit required for filters.
5. Exact live-path daily equity required before quoting production metrics.
6. Methodology labels required for every Sharpe.
7. No more nearby Turtle filters until T65/T67/T68 are complete.

---

## Key Insight

The edge is probably real. The production-readiness story is not. The next breakthrough is not another Turtle parameter; it is making live bot code, harnesses, HOF, reports, and risk controls all tell the same truth.
