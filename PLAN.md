# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-05 21:10 UTC — T69 CANDIDATE REJECTED / EXACT LIVE BOT REMAINS SOURCE OF TRUTH**

## Current Truth

- **Exact as-coded live bot (T65 rerun):** 2.54x / daily account Sharpe 0.94 / MaxDD 28.8% / 298 trades / 1,794 Base5 days.
- **T69 semantic-alignment candidate:** strict prior-window Turtle entry + VOL_LOOKBACK=92 dollar-volume rank gate + size-aware economic accounting produced **1.02x / Sharpe 0.10 / MaxDD 30.8% / 200 trades**. This does **not** close the research/live gap and should **not** be promoted.
- **Research harness result (NOT production):** 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades. It remains useful for signal diagnostics, but it is not the live bot and must not be quoted as production equity.
- **The semantic gap is now partly diagnosed:** copying research-style entry/ranking into the live event/account model does not recover research performance. The research harness likely embeds additional portfolio/timing/accounting assumptions that are not deployable as-is.
- **Sharpe 5+ numbers:** still valid only as per-window walk-forward diagnostics inside their own harnesses, not investor-real account Sharpe.
- **Reports/HOF:** still need cleanup so production headlines come from exact-live daily equity only.
- **Live testnet:** still blocked on Noah's Binance testnet keys (5+ weeks). T53 mock exchange remains the practical bypass once source-of-truth reporting is clean.

## Anti-Spin Read

The last 3–5 sessions spent too much time on same-family Turtle parameter comparisons (ATR rank, AP, LB, hedge threshold/size, weekend filter). That is no longer the highest-value use of time. T69 showed the live/research gap is not solved by another nearby parameter or a naive semantic patch.

Highest-value work is now:
1. **Trust the lab:** regenerate HOF/reports from exact-live T65 only.
2. **Stress the current leader:** risk/abandonment stress on the exact live-bot equity curve.
3. **Execution readiness:** build the mock exchange bypass if credentials remain unavailable.

## Next Tasks (Priority Order)

### T67: Production Metrics Source-of-Truth Regeneration — IMMEDIATE
**Status:** UNBUILT.
- **Problem:** `HALL_OF_FAME.md`, `reports/daily_progress.csv`, progress charts, and strategy docs still mix incompatible strategies and Sharpe methodologies.
- **Action:** Regenerate/update production-facing docs from exact-live T65 only. Keep other metrics explicitly labelled as research diagnostics or graveyard/stale.
- **Output:** Clean production headline: exact live bot = 2.54x / daily account Sharpe 0.94 / MaxDD 28.8%. No `ATR_RANK=24 live bot` row unless explicitly marked stale/rejected.

### T68: Drawdown Abandonment / Risk-of-Ruin Stress Test — NEXT
**Status:** UNBUILT.
- **Problem:** Exact live bot MaxDD is 28.8%; research harness MaxDD is 99.5%. Need operational risk-governance answer before any live deployment.
- **Action:** On exact-live T65 equity/trades, test halt / halve / reduce-risk rules after 20%, 30%, 40%, 50%, 70%, and 85% drawdowns. Report final equity, recovery time, missed top trades, and deployability verdict.
- **Output:** `snapshots/live_bot_abandonment_stress.md`.

### T53: Mock Exchange Bypass — EXECUTION BLOCKER
**Status:** UNBUILT. 5+ weeks overdue.
- Build local HTTP/WS mock exchange seeded from historical 1m parquet; test `src/live/bot.rs` end-to-end without Binance credentials.
- Promote above T61 after T67/T68 if testnet keys remain blocked.

### T61: Binance aggTrades Order-Flow Signal — NEXT TRUE ALPHA
**Status:** UNBUILT / WAIT UNTIL T67+T68.
- Download historical Binance `aggTrades`; aggregate buyer/seller-initiated imbalance into daily confirmation/size features.
- Must include top-trade skip audit so a filter cannot improve average Sharpe by deleting rare convex winners.

## Recently Closed

### T69: Live Bot Semantic Alignment Candidate — REJECTED ✔ (2026-05-05 21:10 UTC)
- Built `examples/live_bot_alignment_candidate.rs`.
- Candidate semantics: strict prior-window Turtle entry, VOL_LOOKBACK=92 top-3 dollar-volume gate, size-aware accounting, exact event-order replay.
- Result: **1.02x / Sharpe 0.10 / MaxDD 30.8% / 200 trades**.
- Verdict: Do **not** patch `src/live/bot.rs` with this candidate. Exact as-coded live bot remains stronger at 2.54x / Sharpe 0.94.

### T65: Exact Live-Bot Source-of-Truth Harness — DONE ✔ (2026-05-05)
- `examples/live_bot_exact_equity.rs` built and rerun.
- Exact as-coded result: **2.54x / Sharpe 0.94 / MaxDD 28.8% / 298 trades / 1,794 days**.
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
