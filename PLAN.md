# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-06 00:20 UTC — T67/T68 DONE / EXACT LIVE BOT REPORTING CLEANED**

## Current Truth

- **Exact as-coded live bot (T65/T67 rerun):** 2.55x / daily account Sharpe 0.94 / MaxDD 28.8% / 298 trades / 1,795 Base5 days.
- **T69 semantic-alignment candidate:** strict prior-window Turtle entry + VOL_LOOKBACK=92 dollar-volume rank gate + size-aware economic accounting produced **1.02x / Sharpe 0.10 / MaxDD 30.8% / 200 trades**. This does **not** close the research/live gap and should **not** be promoted.
- **Research harness result (NOT production):** 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades. It remains useful for signal diagnostics, but it is not the live bot and must not be quoted as production equity.
- **The semantic gap is now partly diagnosed:** copying research-style entry/ranking into the live event/account model does not recover research performance. The research harness likely embeds additional portfolio/timing/accounting assumptions that are not deployable as-is.
- **Sharpe 5+ numbers:** still valid only as per-window walk-forward diagnostics inside their own harnesses, not investor-real account Sharpe.
- **Reports/HOF:** cleaned by T67; production headlines now come from exact-live daily equity only.
- **Risk governance:** T68 found only the 20% DD threshold breaches; 30%+ never trigger in sample. A 20% hard-abandon rule would have stopped on 2022-09-13 at 1.27x and missed 3 of the top-10 winners.
- **Live testnet:** still blocked on Noah's Binance testnet keys (5+ weeks). T53 mock exchange is now the practical bypass.

## Anti-Spin Read

The last 3–5 sessions spent too much time on same-family Turtle parameter comparisons (ATR rank, AP, LB, hedge threshold/size, weekend filter). That is no longer the highest-value use of time. T69 showed the live/research gap is not solved by another nearby parameter or a naive semantic patch.

Highest-value work is now:
1. **Execution readiness:** build the mock exchange bypass if credentials remain unavailable.
2. **Broaden edge discovery:** only after execution readiness, begin T61 aggTrades order-flow work.
3. **Trust maintenance:** keep HOF/reports exact-live-only; do not resurrect old WF/research headlines.

## Next Tasks (Priority Order)

### T53: Mock Exchange Bypass — EXECUTION BLOCKER / IMMEDIATE
**Status:** UNBUILT. 5+ weeks overdue.
- Build local HTTP/WS mock exchange seeded from historical 1m parquet; test `src/live/bot.rs` end-to-end without Binance credentials.
- This is now higher value than more backtests because T67/T68 closed the current trust/risk reporting debt.

### T61: Binance aggTrades Order-Flow Signal — NEXT TRUE ALPHA
**Status:** UNBUILT / WAIT UNTIL T53 READINESS WORK.
- Download historical Binance `aggTrades`; aggregate buyer/seller-initiated imbalance into daily confirmation/size features.
- Must include top-trade skip audit so a filter cannot improve average Sharpe by deleting rare convex winners.

## Recently Closed

### T68: Drawdown Abandonment / Risk-of-Ruin Stress Test — DONE ✔ (2026-05-06)
- Built `examples/t68_abandonment_stress.rs` using exact-live T65/T67 equity and trades.
- Baseline: **2.55x / MaxDD 28.8% / 298 trades**.
- Only the 20% drawdown threshold breaches; 30%+ thresholds never trigger in-sample.
- 20% hard abandonment triggers on 2022-09-13, waits 451 days for baseline recovery, and leaves final equity at **1.27x** while missing 3 of the top-10 winners (1.42x combined multiplier).
- Verdict: use 20% DD as a human review trigger, but do not auto-abandon below 30% without live/testnet evidence.

### T67: Production Metrics Source-of-Truth Regeneration — DONE ✔ (2026-05-06)
- `HALL_OF_FAME.md`, `reports/daily_progress.csv`, `scripts/gen_hof.py`, `scripts/run_daily_progress.sh`, and `charts/live_bot_exact_equity.png` now use exact-live T65/T67 only.
- Old mixed methodology rows preserved at `reports/daily_progress_PRE_T67_STALE.csv` and removed from the production report.
- Clean headline: **exact live bot = 2.55x / daily account Sharpe 0.94 / MaxDD 28.8% / 298 trades / 1,795 days**.

### T69: Live Bot Semantic Alignment Candidate — REJECTED ✔ (2026-05-05 21:10 UTC)
- Built `examples/live_bot_alignment_candidate.rs`.
- Candidate semantics: strict prior-window Turtle entry, VOL_LOOKBACK=92 top-3 dollar-volume gate, size-aware accounting, exact event-order replay.
- Result: **1.02x / Sharpe 0.10 / MaxDD 30.8% / 200 trades**.
- Verdict: Do **not** patch `src/live/bot.rs` with this candidate. Exact as-coded live bot remains stronger at 2.55x / Sharpe 0.94.

### T65: Exact Live-Bot Source-of-Truth Harness — DONE ✔ (2026-05-05)
- `examples/live_bot_exact_equity.rs` built and rerun.
- Exact as-coded result: **2.55x / Sharpe 0.94 / MaxDD 28.8% / 298 trades / 1,795 days**.
- Critical drift found: VOL_LOOKBACK=92 unused by bot.rs; entry is current-inclusive/equality-permissive; live UI accounting ignores trade size, though T65 reports economic account equity.

### T69: REGIME_LOOKBACK Extensive Sweep — DONE ✔ (2026-05-05)
- LB=41/42/44/45 lie inside a tight plateau depending on harness variant; no robust production edge from another LB tweak.

### T66: Hedge Size Mult Sweep — DONE ✔ (2026-05-05)
- `HEDGE_SIZE_MULT` 0.70 → 0.40 via 13-value × 9-universe × 7-window sweep.
- Risk overlay tuning, not alpha.

### T62: Weekend Effect Filter — REJECTED ✔ (2026-05-05)
- Weekend entries are valuable, not inferior. Do not revisit without a new mechanism.

## Remaining Blocker

Live testnet (Noah's API keys, 5+ weeks). T53 mock exchange bypasses this.
