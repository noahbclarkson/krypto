# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-06 00:48 UTC — CRITIQUE CYCLE COMPLETE / T70 SEMANTIC GAP ADDED**

## Current Truth

- **Exact as-coded live bot (T65/T67):** 2.55x / daily account Sharpe 0.94 / MaxDD 28.8% / 298 trades / 1,795 Base5 days.
- **Semantic gap UNDERDIAGNOSED — 70x equity difference unexplained:** Research harness = 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades. Live bot (exact) = 2.54x / Sharpe 0.94 / MaxDD 28.8% / 298 trades. T69 semantic patch (strict prior-window Turtle + VL ranking + size-aware accounting) made it WORSE → 1.02x. **We don't know which research-harness-validated parameters (EP=21, AP=17, LB=41, VL=92) actually apply to the live bot path.**
- **Top-trade concentration risk:** Top 10 trades = 91.5% of log return. Top 5 = 57.8%. A filter that accidentally excludes 2-3 winners reduces 2.55x to ~1.5x.
- **Hedge overlay is inert dead code:** HEDGE_ATR_PCT (101 values, all identical) and HEDGE_SIZE_MULT (13 values, pure risk dial) are documented but not used in the validated live execution path.
- **T68 risk governance:** 20% DD threshold is human review trigger only. 30%+ never breached in-sample. 20% hard abandonment leaves final equity at 1.27x and misses 3 top-10 winners.
- **Reports/HOF:** cleaned by T67; production headlines now come from exact-live daily equity only.
- **Live testnet:** still blocked on Noah's Binance testnet keys (5+ weeks). T53 mock exchange is the practical bypass.

## Anti-Spin Read

The last 3–5 sessions spent too much time on same-family Turtle parameter comparisons (ATR rank, AP, LB, hedge threshold/size, weekend filter). That is no longer the highest-value use of time. T69 showed the live/research gap is not solved by another nearby parameter or a naive semantic patch.

Highest-value work is now:
1. **T53 execution readiness:** build the mock exchange bypass.
2. **T70 semantic gap diagnosis:** understand WHY research produces 176x vs live 2.55x.
3. **T61 aggTrades order-flow:** new information source after execution readiness.

## Next Tasks (Priority Order)

### T53: Mock Exchange Bypass — EXECUTION BLOCKER / IMMEDIATE
**Status:** UNBUILT. 5+ weeks overdue.
- `mock_live_bot.rs` (461 lines) and `mock_live_bot_v2.rs` (503 lines) exist as stubs — NOT yet end-to-end wire replacements for Binance WebSocket.
- Goal: local HTTP/WS mock server seeded from historical 1m parquet; tests `src/live/bot.rs` end-to-end without Binance credentials.
- Must wire `src/live/bot.rs` → mock exchange → verify it produces same signals as T65 harness on the same data.
- This unblocks real execution feedback: fills, slippage, order state, disconnect/reconnect.

### T70: Semantic Gap Mechanism Audit — CRITICAL (explains 70x equity gap)
**Status:** UNBUILT. The gap (research 176.79x vs live 2.54x) is NOT explained.
- **Hypothesis A (exit):** dual Chandelier exit vs Turtle-only exit produces very different trade durations and compounding. Test: run live bot with dual Chandelier exit vs Turtle-only exit on same data, measure equity impact per trade.
- **Hypothesis B (sizing):** equal-size positions vs research harness dollar-volume ranking. Test: run live bot with and without VL ranking on same data.
- **Hypothesis C (accounting):** economic mark-to-market vs realized-only equity model. Test: measure the difference in daily equity between the two accounting approaches.
- Goal: know which research-harness-validated parameters (EP=21, AP=17, LB=41, etc.) actually transfer to the live path.
- Must include top-10 winner conditions audit: which trades produced the convex winners, and would any plausible filter have excluded them?
- Top-winner audit guards against accidentally building filters that improve average Sharpe but destroy convex tail returns.

### T61: Binance aggTrades Order-Flow Signal — TRUE ALPHA (after T53/T70)
**Status:** UNBUILT / WAIT UNTIL T53 AND T70.
- Download historical Binance `aggTrades`; aggregate buyer/seller-initiated imbalance into daily confirmation/size features.
- Genuinely new information dimension — all recent work has been price-only parameter tuning.
- Must pass top-trade skip audit (filters cannot improve average Sharpe by deleting rare convex winners).

## Recently Closed

### T68: Drawdown Abandonment / Risk-of-Ruin Stress Test — DONE ✔ (2026-05-06)
- Built `examples/t68_abandonment_stress.rs` using exact-live T65/T67 equity and trades.
- Baseline: **2.55x / MaxDD 28.8% / 298 trades**.
- Only the 20% drawdown threshold breaches; 30%+ thresholds never trigger in-sample.
- 20% hard abandonment triggers on 2022-09-13, waits 451 days for baseline recovery, and leaves final equity at **1.27x** while missing 3 of the top-10 winners (1.42x combined multiplier).
- Verdict: use 20% DD as a human review trigger, but do not auto-abandon below 30% without live/testnet evidence.

### T67: Production Metrics Source-of-Truth Regeneration — DONE ✔ (2026-05-06)
- `HALL_OF_FAME.md`, `reports/daily_progress.csv`, `scripts/gen_hof.py`, and `scripts/run_daily_progress.sh` now use exact-live T65/T67 only.
- Old mixed methodology rows preserved at `reports/daily_progress_PRE_T67_STALE.csv` and removed from production report.
- Clean headline: **exact live bot = 2.55x / daily account Sharpe 0.94 / MaxDD 28.8% / 298 trades / 1,795 days**.

### T69: Live Bot Semantic Alignment Candidate — REJECTED ✔ (2026-05-05)
- Built `examples/live_bot_alignment_candidate.rs`.
- Candidate semantics: strict prior-window Turtle entry, VOL_LOOKBACK=92 top-3 dollar-volume gate, size-aware accounting, exact event-order replay.
- Result: **1.02x / Sharpe 0.10 / MaxDD 30.8% / 200 trades** — WORSE than exact as-coded bot (2.54x).
- Verdict: do **not** patch `src/live/bot.rs` with this candidate. Exact as-coded live bot remains stronger at 2.54x.
- **New insight (2026-05-06):** the gap is NOT just semantic patching. T70 is needed to diagnose the actual mechanism of the 70x equity difference.

### T69: REGIME_LOOKBACK Extensive Sweep — DONE ✔ (2026-05-05)
- LB=41/42/44/45 lie inside a tight plateau depending on harness variant; no robust production edge from another LB tweak.

### T67: HEDGE_ATR_PCT Hyperopt — NULL RESULT ✔ (2026-05-05)
- `examples/t67_hedge_atr_pct_extensive.rs` — 101 values × 9 universes × 7 windows = 6,363 sims.
- Result: all 101 values produce IDENTICAL results (56/63 pass, Sharpe 6.941, 689 trades).
- HEDGE_ATR_PCT is a dead/inert parameter in the validated live path. No further work needed.

### T66: Hedge Size Mult Sweep — DONE ✔ (2026-05-05)
- `HEDGE_SIZE_MULT` 0.70 → 0.40 via 13-value × 9-universe × 7-window sweep.
- Pure risk dial, not alpha. Size mult controls position size during high-vol regimes.

### T62: Weekend Effect Filter — REJECTED ✔ (2026-05-05)
- Weekend entries are valuable, not inferior. Do not revisit without a new mechanism.

## New Critical Finding: Semantic Gap Mechanism Unknown (T70)

The 70x equity gap (research 176.79x vs live 2.54x) is NOT explained by T69 semantic patching. The candidate patch made results WORSE (1.02x). The mechanism must be systematically diagnosed (T70) before any research-harness-validated parameters can be trusted for the live path.

## Remaining Blocker

Live testnet (Noah's API keys, 5+ weeks). T53 mock exchange bypasses this.