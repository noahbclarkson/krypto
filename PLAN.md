# PLAN.md — Krypto Research and Execution Plan

<<<<<<< HEAD
**State: 2026-05-05 16:05 UTC — CRITIQUE COMPLETE / SOURCE-OF-TRUTH FIRST**

## Current Truth

- **Live bot code path:** Turtle-only breakout + Turtle ATR exit + AP17/LB42/T5 ATR_RANK gate + VL92 volume ranking + USDT hedge overlay.
- **Current production params in code:** `EP=21`, `TURTLE_ATR_P=24`, `TURTLE_ATR_M=2.0`, `HOLD_MAX=12`, `POSITION_CAP=3`, `ATR_ENTRY_MULT=0.00`, `REGIME_ATR_P=17`, `REGIME_LOOKBACK=42`, `ATR_RANK_THRESHOLD=5.0`, `VOL_LOOKBACK=92`, `HEDGE_ATR_PCT=0.45`, `HEDGE_SIZE_MULT=0.40`.
- **Validated Turtle-only research path:** 176.79x / daily Sharpe 3.29 / MaxDD 99.5% / 156 trades, but excludes current hedge overlay.
- **Validated live-compatible WF path with hedge:** 58/63 pass (92.1%), WF Sharpe 7.079, return +114.6%, DD 21.2%, but this is per-window WF scoring, not account-level daily Sharpe.
- **Weekend filter:** rejected. Do not revisit without a new mechanism.
- **Hedge overlay:** likely useful as a risk dial, but not alpha. Needs exact-live daily equity before being quoted as production truth.
- **Reports/HOF:** stale and inconsistent. `daily_progress.csv` still contains misleading `ATR_RANK=24` live-bot labels; `HALL_OF_FAME.md` still cites old AP=12/dual-exit production evidence.
- **Live testnet:** blocked on Noah's Binance testnet keys for 5+ weeks. T53 mock exchange remains the practical bypass after trust work.

## Brutal Critique Summary

The last five commits are not worthless, but they mostly validate or reject nearby Turtle variants rather than move the bot toward autonomous trading. T62 was a good falsification. Hedge threshold/size sweeps are useful but are risk-overlay tuning, not new alpha. The daily progress commit is actively risky because it refreshed charts while leaving production labels/methodologies inconsistent.

Sharpe 5.0+ numbers are mostly walk-forward per-window metrics. They are valid diagnostics inside the harness, but they are not investor-real account Sharpe. The honest production question is still unanswered: **what exactly would `src/live/bot.rs`, as coded today, have done historically?**

Biggest blind spot: **production source-of-truth drift**, followed by **drawdown survivability**. A strategy with 99.5% MaxDD cannot be called low-risk until we know whether practical abandonment/risk-cut rules destroy the edge.

## Next Tasks (Priority Order)

### T65: Exact Live-Bot Source-of-Truth Harness — IMMEDIATE
**Status:** UNBUILT.
- **Problem:** Current research/equity harnesses do not provide one canonical daily-equity answer for the exact `src/live/bot.rs` code path.
- **Action:** Build/refactor one harness matching live bot behavior exactly: Turtle entry, AP17/LB42/T5 ATR_RANK, VL92 ranking, Turtle ATR exit, cap=3, taker fees, hedge pct=0.45, hedge size=0.40, same sequencing.
- **Output:** `snapshots/live_bot_exact_equity.md` + CSV with equity, daily Sharpe, MaxDD, trades, yearly table, top-trade attribution, parameter table, and explicit methodology labels.
- **Accept gate:** Numbers must be traceable to the same constants as `src/live/config.rs`; no stale AP/T/VL/hedge comments allowed.

### T67: Production Metrics Source-of-Truth Regeneration — AFTER T65
**Status:** UNBUILT. Renumbered because commit `b8a9a70b` used T66 for hedge-size sweep.
- **Problem:** `HALL_OF_FAME.md`, `reports/daily_progress.csv`, progress charts, and strategy docs quote incompatible strategies and Sharpe methodologies.
- **Action:** Regenerate/update HOF and daily progress from T65 only. Add/keep methodology labels: daily compounded account Sharpe, per-window WF Sharpe, attribution Sharpe, milestone-aggregated Sharpe.
- **Output:** Clean production headline with no dual-exit/live-path confusion and no `ATR_RANK=24` live-bot row unless explicitly marked graveyard/stale.

### T68: Drawdown Abandonment / Risk-of-Ruin Stress Test — BEFORE NEW ALPHA
**Status:** UNBUILT.
- **Problem:** 99.5% MaxDD is operationally catastrophic. The strategy may only work if the operator never cuts risk through near-total drawdown.
- **Action:** On T65 equity/trades, test capital cut, halt, and risk-reduction rules after 50%, 70%, 85%, and 95% drawdowns. Report final equity, recovery time, missed top trades, and whether the edge survives realistic abandonment.
- **Output:** `snapshots/live_bot_abandonment_stress.md` with deployability verdict.

### T61: Binance aggTrades Order-Flow Signal — NEXT TRUE ALPHA
**Status:** UNBUILT / WAIT UNTIL T65–T68.
- Download historical Binance `aggTrades` and aggregate buyer/seller-initiated imbalance into daily confirmation/size features.
- Must include top-trade skip audit so a filter cannot improve average Sharpe by deleting rare convex winners.

### T53: Mock Exchange Bypass — EXECUTION BLOCKER
**Status:** UNBUILT. 5+ weeks overdue.
- Build local HTTP/WS mock exchange seeded from historical 1m parquet and test `src/live/bot.rs` end-to-end without Binance credentials.
- Promote above T61 if Noah's testnet keys remain blocked after T65/T67/T68.

## Recently Closed

### T66: Hedge Size Mult Sweep — DONE ✔ (2026-05-05)
- `HEDGE_SIZE_MULT` changed 0.70 → 0.40 after 13-value × 9-universe × 7-window sweep.
- Treat as risk overlay tuning, not new alpha.

### T62: Weekend Effect Filter — REJECTED ✔ (2026-05-05)
- NO_FILTER: 58/63 pass, Sharpe 7.079, Ret +114.6%, DD 21.2%.
- NO_WEEKEND: 56/63 pass, Sharpe 6.489, Ret +108.9%, DD 23.0%.
- Weekend entries are not structurally inferior; skipping them removes edge.
=======
**State: 2026-05-05 20:05 UTC — SEMANTIC DRIFT CRISIS. T65 built but gap UNCLOSED. Research ≠ Live bot.**

## Current Truth

- **T65 exact live-bot result:** 2.54x / daily account Sharpe 0.94 / MaxDD 28.8% / 301 trades / 1,794 Base5 days.
- **Research harness result (NOT the live bot):** 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades — from `examples/turtle_only_equity.rs`, which uses different signal semantics.
- **The 69x equity gap is semantic, not calibration.** Research harness uses strict prior-window Turtle, VL92 volume ranking, size-aware accounting, no hedge overlay. Live bot uses current-inclusive/equality-permissive entry, no VL ranking, size-agnostic accounting, USDT hedge overlay.
- **Every Turtle param optimized on the research harness (EP, AP, LB, VL, CHAND, ATR, HOLD_MAX, hedge pct/size, weekend filter) may be irrelevant to the live bot** because the signal path differs.
- **LB=45 (T69 result) = zero improvement over LB=42** — identical Sharpe 6.188 across all metrics. Confirmation only.
- **Reports/HOF are stale** — mix Sharpe methodologies, cite T=24 live-bot row that doesn't match current config.
- **Live testnet:** still blocked on Noah's Binance testnet keys (5+ weeks). T53 mock exchange remains the practical bypass.

## Brutal Critique Summary (This Session)

The last 5 commits are mixed quality. The two most important were: (1) T65 — genuinely useful, exposed the critical semantic drift; (2) T69 LB=45 hyperopt — zero improvement, confirmation only, another same-family plateau catch.

**The biggest blind spot is not fees, not bull-market bias, not over-trading.** It is that we spent 2+ years optimizing a research harness that does not accurately represent `src/live/bot.rs`. Every parameter we validated may be irrelevant to the bot. The research equity of 176.79x is not production-ready because it is a different system.

Sharpe 5.0+ numbers are valid per-window diagnostics in their own harnesses but are NOT investor-real account Sharpe. The only honest live bot number is T65's 2.54x / Sharpe 0.94 / MaxDD 28.8%.

## Next Tasks (Priority Order)

### T69: Live Bot Semantic Alignment — IMMEDIATE
**Status:** UNBUILT.
- **Problem:** T65 proved research harness (176.79x) ≠ live bot (2.54x). Every Turtle param optimized on research harness may be irrelevant to live bot.
- **Option A (preferred):** Patch `src/live/bot.rs` to use: (1) strict prior-window Turtle entry, (2) VL92 volume ranking, (3) size-aware accounting. Align bot to research harness → rerun T65 → see if equity gap closes.
- **Option B:** Accept 2.54x as honest production number. Regenerate HOF/reports from T65 only. Stop quoting research equity as production truth.
- **Output:** Audited code diff + rerun `snapshots/live_bot_exact_equity.md`, then regenerate HALL_OF_FAME.md and `reports/daily_progress.csv` from the single chosen source.

### T68: Drawdown Abandonment / Risk-of-Ruin Stress Test — BEFORE NEW ALPHA
**Status:** UNBUILT.
- **Problem:** 99.5% MaxDD (research) vs 28.8% MaxDD (live bot). Both survivable in backtest, but what happens if a human cuts risk during drawdown?
- **Action:** On T65 exact equity: test capital cut / halt / risk-reduction at 50%, 70%, 85% drawdowns. Report final equity, recovery time, missed top trades, deployability verdict.
- **Output:** `snapshots/live_bot_abandonment_stress.md`.

### T61: Binance aggTrades Order-Flow Signal — NEXT TRUE ALPHA
**Status:** UNBUILT / WAIT UNTIL T69+T68.
- **New information:** Download historical Binance `aggTrades`; aggregate buyer/seller-initiated imbalance into daily confirmation/size features.
- **Must pass top-trade skip audit** (T63: top 10 trades = 91.8% of log-return; a filter that removes them destroys edge even if average Sharpe improves).
- **Why:** all recent work is price-only threshold tuning of the same Turtle signal. New microstructure information is the only path to genuinely new edge.

### T53: Mock Exchange Bypass — EXECUTION BLOCKER
**Status:** UNBUILT. 5+ weeks overdue.
- Build local HTTP/WS mock exchange seeded from historical 1m parquet; test `src/live/bot.rs` end-to-end without Binance credentials.
- Promote above T61 if testnet keys remain blocked after T69/T68.
>>>>>>> b656acd (docs: critique and plan update — semantic drift crisis, T65/T69 findings)

## Recently Closed

### T65: Exact Live-Bot Source-of-Truth Harness — DONE ✔ (2026-05-05 18:10 UTC)
- `examples/live_bot_exact_equity.rs` built.
- Exact as-coded result: **2.54x / Sharpe 0.94 / MaxDD 28.8% / 301 trades / 1,794 days**.
- Critical drift found: VOL_LOOKBACK=92 unused by bot.rs; entry is current-inclusive/equality-permissive; BotState accounting ignores trade size.
- Research equity (176.79x) is NOT the live bot — semantic gap confirmed.

### T69: REGIME_LOOKBACK LB=42→41 Extensive Sweep — DONE ✔ (2026-05-05)
- LB=45 = LB=42 = LB=44 on all metrics (Sharpe 6.188, identical). Zero improvement.
- LB=45 plateau confirmed. No production change needed.

### T66: Hedge Size Mult Sweep — DONE ✔ (2026-05-05)
- `HEDGE_SIZE_MULT` 0.70 → 0.40 via 13-value × 9u × 7w sweep.
- Risk overlay tuning, not alpha.

### T62: Weekend Effect Filter — REJECTED ✔ (2026-05-05)
- NO_FILTER: 58/63 pass, Sharpe 7.079. NO_WEEKEND: 56/63, Sharpe 6.489.
- Weekend entries are valuable, not inferior. REJECTED.

### T63: Per-Trade PnL Attribution — DONE ✔ (2026-05-05)
- 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades.
- Top 5 trades = 38.2% of log-return; top 10 = 60.9%. Real convex edge, not lottery.
- Fee drag 4.7% of gross only. Win rate 53.8%, W/L 2.02x.
- Research harness path only — does not include live bot's USDT hedge overlay.

### T64: Regime Sharpe Decomposition — DONE ✔ (2026-05-05)
- Bull/bear Sharpe balanced (1.80/1.80). Not bear-only.
- Weak buckets: chop Q1 Sharpe 1.07, trend-vol Q4 Sharpe 1.16.
- ATR_RANK=5 is mixed: improves bear/trend-vol but cuts equity and worsens attribution MaxDD.

## COMPLETED (Historical)

| Task | Status | Key Finding |
|------|--------|-------------|
| ATR_RANK=24 | GRAVEYARD | Held-out: 10/22 pass/-0.964 Sharpe vs T=5 14/22/+0.664 |
| ATR_ENTRY_MULT all | REJECTED | EM=0.00 wins definitively |
| EP=24 | REVERTED | Held-out: 25/29 vs EP=21 27/29 |
| Short-side sleeve | GRAVEYARD | 37.5% pass vs 69.1% guardrail |
| SIZE_MULT overlay | INERT | Pure risk knob, no alpha |
| Funding Rate Regime Filter | GRAVEYARD | 4,590 runs, pass NEVER improves |
| Donchian sleeve | REJECTED | 63% < 69.1% guardrail |
| 4h Multi-Timeframe Turtle | GRAVEYARD | 1/20 pass — structural timeframe incompatibility |
| BollingerReversion | GRAVEYARD | 0/288 OOS — signal actively harmful |
| ATR-norm position sizing | REJECTED | Equal capital optimal; ATR-norm inverts vol ranking |
| CTREND 25% sleeve | REJECTED | Sharpe destroyed 1.38→0.33 |
| A/D static sleeve | REJECTED | Below-random win rate |

## Remaining Blocker

Live testnet (Noah's API keys, 5+ weeks). T53 mock exchange bypasses this.