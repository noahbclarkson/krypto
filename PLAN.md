# PLAN.md — Krypto Research and Execution Plan

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

## COMPLETED (Recent)

### T63: Per-Trade PnL Attribution — DONE ✔ (2026-05-05 12:20 UTC)
**Results:** 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades (Turtle-only AP17/T5/VL92, no live-bot hedge overlay)
- Fee drag: 32.6% additive, 4.7% of gross.
- Win rate: 53.8%, avg win +13.76%, avg loss -6.81%, W/L 2.02x.
- Top 5 trades explain 38.2% of log-return; equity without top 5 remains 24.51x.
- Top 10 trades explain 60.9%; equity without top 10 is 7.58x.
- Verdict: real convex trend-following edge, not single-trade mirage. But missing rare breakout winners can destroy performance.
- Files: `examples/t63_trade_attribution.rs`, `snapshots/t63_trade_attribution.{md,csv}`.

### T59: Turtle-Only Daily Equity Curve — DONE ✔ (2026-05-05, updated after VL92)
**Results:** 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades / 1794 days (AP17/T5/VL92)
- Earlier T59 112.27x / Sharpe 3.14 used VL96 before the dense AP17 sweep promoted VL92.
- Live path research equity confirmed for Turtle-only/AP17/T5/VL92, but exact `src/live/bot.rs` remains unconfirmed because of hardcoded hedge overlay.

### T60: Per-Year Performance Decomposition — DONE ✔ (2026-05-05)
**Status:** COMPLETE via Turtle-only equity output.
- 2022 mega-trend remains the dominant contributor to compounded equity.
- Use exact-live T65 harness before quoting final production yearly metrics.

### T64: Regime Sharpe Decomposition — DONE ✔ (2026-05-05 09:20 UTC)
**Results:** Production T=5 attribution = 114.19x / Sharpe 1.68 / MaxDD 51.2% / 154 trades. T=0 no-gate control = 206.95x / Sharpe 1.75 / MaxDD 45.9% / 189 trades.
- Bull/bear Sharpe balanced: 1.80 / 1.80. Not purely bear-only.
- Weak buckets are vol regimes: chop Q1 1.07, trend-vol Q4 1.16.
- ATR_RANK=5 is mixed: slight bear Sharpe lift and trend-vol lift, but lower equity/trades and worse attribution DD. T=0 needs held-out/live-path validation before any production change.
- Files: `examples/regime_sharpe_decomposition.rs`, `snapshots/regime_sharpe_decomposition.{csv,md}`.

### T62 (AP=17 held-out validation): DONE ✔ (2026-05-04)
- AP=17: 4/4 pass, Sharpe 7.715, equity 1.9481x, DD 21.3% ← WINNER
- AP=63: 4/4 pass, Sharpe 5.721, equity 1.3976x, DD 26.6% ← rejected
- AP=12: 2/4 pass ← rejected
- Config.rs updated to REGIME_ATR_PERIOD=17. AP question CLOSED.

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