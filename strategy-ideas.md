# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-06 00:48 UTC. Critique cycle complete. T65/T67/T68 closed. T70 (semantic gap mechanism audit) is now the highest-priority unbuilt task. T61 next after T53/T70.*

---

## Critical Alerts

### T70: Semantic Gap Mechanism Unknown — 70x Equity Difference Unexplained

Research harness = 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades.
Live bot (exact) = 2.54x / Sharpe 0.94 / MaxDD 28.8% / 298 trades.

T69 semantic patch (strict prior-window Turtle + VL ranking + size-aware accounting) made it WORSE → 1.02x. The gap is NOT a simple semantic difference. The mechanism must be diagnosed before any research-harness-validated parameters (EP=21, AP=17, LB=41, VL=92) can be trusted for the live path.

Three hypotheses:
- **(A) Exit difference:** dual Chandelier vs Turtle-only produces very different compounding (156 trades × long holds vs 298 trades × short holds)
- **(B) Sizing difference:** equal-size vs dollar-volume ranking changes which symbols get capital
- **(C) Accounting model:** economic mark-to-market vs realized-only equity

### Top-Trade Concentration Risk Is Underappreciated

Top 10 trades = 91.5% of log return. Top 5 = 57.8%. A filter that accidentally excludes 2-3 winners reduces 2.55x to ~1.5x. Every new entry filter MUST pass a top-10 winner skip audit.

---

## Top 3 Most Promising Unbuilt Ideas

### T70: Semantic Gap Mechanism Audit — CRITICAL (new 2026-05-06)
**Status:** UNBUILT.
- **Problem:** 70x equity gap between research harness (176.79x) and live bot (2.54x) is not explained.
- **Action:** Systematic decomposition of three hypotheses (exit/sizing/accounting). Run pairwise comparisons on same data with single variable changed at a time.
- **Output:** per-hypothesis equity delta, top-10 winner conditions document, param transferability verdict.
- **Why:** Without understanding the gap mechanism, every research-harness-validated parameter is potentially irrelevant to the live path.

### T53: Mock Exchange Bypass — EXECUTION BLOCKER (5+ weeks overdue)
**Status:** UNBUILT.
- `mock_live_bot.rs` (461 lines) and `mock_live_bot_v2.rs` (503 lines) exist as stubs — not yet end-to-end wire replacements.
- **Problem:** Live testnet blocked on Noah's Binance testnet keys. No execution feedback without them.
- **Action:** Build local HTTP/WS mock seeded from historical 1m parquet. Wire `src/live/bot.rs` → mock → verify against T65 harness signals.
- **Output:** End-to-end test of `src/live/bot.rs` decision path, fills, slippage, order state, disconnect/reconnect.
- **Why:** Unblocks real execution feedback without API credentials.

### T61: Binance aggTrades Order-Flow Signal — TRUE ALPHA
**Status:** UNBUILT / START AFTER T53 AND T70.
- Download historical Binance `aggTrades`; aggregate taker buy/seller-initiated imbalance into daily confirmation/size features.
- Genuinely new information dimension — all recent work is price-only parameter tuning.
- **Guardrail:** Must pass top-trade skip audit. Reject if it filters out rare convex winners even when average Sharpe improves.

---

## Infrastructure / Trust Tasks

- [x] **T65**: Exact live-bot source-of-truth harness — DONE (2026-05-05)
- [x] **T67**: Regenerate HOF/reports from T65 only — DONE (2026-05-06)
- [x] **T68**: Drawdown abandonment / risk-of-ruin stress — DONE (2026-05-06)
- [ ] **T70**: Semantic gap mechanism audit — NEW (highest priority)
- [ ] **T53**: Local mock exchange bypass — UNBUILT (5+ weeks overdue)
- [ ] **T55**: LOB NOBI collection/persistence — not ready until data exists

---

## Resolved / Closed

- **T65:** done; exact live bot 2.55x / Sharpe 0.94 / MaxDD 28.8% / 298 trades. VOL_LOOKBACK=92 unused by bot.rs — gap confirmed.
- **T67:** done; HOF/reports regenerated from exact-live only. Old mixed rows at `reports/daily_progress_PRE_T67_STALE.csv`.
- **T68:** done; 20% DD human review trigger only, 30%+ never breached in-sample. Hard abandonment leaves 1.27x and misses 3 top-10 winners.
- **T69 semantic patch:** REJECTED; 1.02x (worse than live bot 2.54x). Gap is not a semantic patching problem.
- **T67 HEDGE_ATR_PCT:** NULL result; all 101 values identical (56/63 pass, Sharpe 6.941). Inert dead code.
- **AP=63 anti-overfit violation:** resolved; AP=17 promoted after held-out validation.
- **T59 Turtle-only equity:** 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades — research diagnostic only.
- **T63 per-trade attribution:** top 5 = 57.8%, top 10 = 91.5% of log return. Convex winners matter enormously.
- **T66 hedge-size sweep:** HEDGE_SIZE_MULT 0.70 → 0.40 — pure risk dial, not alpha.
- **T62 weekend filter:** REJECTED; weekend entries are valuable, not inferior.

---

## New Concepts Added From Critique (2026-05-06)

### C8: Semantic Gap Mechanism Audit
Systematic decomposition of why research harness produces 176.79x vs live bot 2.54x. Test each hypothesis (exit, sizing, accounting) in isolation. The goal is not to close the gap but to understand it, so we know which research parameters are trustworthy for the live path.

### C9: Top-Winner Conditions Audit
For the top 10 trades by log-return in the exact-live replay, document: (a) entry date/symbol, (b) market regime, (c) ATR percentile at entry, (d) whether a plausible filter would have excluded them. Quantifies the risk of over-filtering.

### C10: Inert Code Cleanup Rule
A parameter that produces identical results across its full logical range (HEDGE_ATR_PCT = 101 values all identical) is dead code. Document it as experimental-only or remove it. Confusing documentation about non-contributing overlays is a long-term maintenance risk.

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

### C6: Semantic Gap Trap
The research harness and `src/live/bot.rs` are semantically different systems — different entry conditions, no VL ranking in the live bot, size-agnostic accounting, different hedge overlay. Every Turtle param validated on the research harness may be irrelevant to the live bot. T69 proved this: patching toward the research harness made results worse.

### C7: Two-Option T69 Decision Framework
When source-of-truth gap is confirmed, only two valid paths:
- **Option A:** Patch the live bot to match the validated research harness, rerun exact-live harness. If equity gap closes, research equity becomes production equity.
- **Option B:** Accept the as-coded live bot equity as the honest production number. Regenerate all reports from that single source. Never mix research-harness and live-bot numbers.

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
7. No more nearby Turtle filters until T70/T53 are complete.

---

## Key Insight

The edge is probably real. The production-readiness story is fragmented. The semantic gap between research (176.79x) and live (2.54x) is not a parameter problem — it is a structural difference we haven't explained. T70 is the highest-priority task to resolve this before any more parameter tuning.