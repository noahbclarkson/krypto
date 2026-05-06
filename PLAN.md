# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-06 04:05 UTC — CRITIQUE FINDINGS / T70 STILL UNBUILT / T71 NEW**

## Critical New Findings

### Finding 1: "Freshness Cooldown" ≠ T70 Semantic Gap Audit
Last session claimed to do T70 but ran a freshness-cooldown parameter sweep instead. The actual T70 (exit/sizing/accounting hypothesis decomposition explaining the 70x equity gap) remains **UNBUILT**. Three more documentation commits have been written about doing T70, but T70 has not been executed.

### Finding 2: VOL_LOOKBACK=92 Configured But Never Used by bot.rs
`src/live/bot.rs` does not use VOL_LOOKBACK=92 in its entry logic. The research harness's vol-adaptive ranking is completely absent from the live bot's execution path. This is the strongest candidate for explaining why the research harness produces 176.79x vs the live bot's 2.54x. The uncommitted bot.rs changes may include progress on this — but nothing is committed.

### Finding 3: 165+ Working Tree Files Uncommitted
Modified (not staged): data cache, 160+ example files, src/live/bot.rs, src/live/config.rs, src/live/executor.rs, src/live/mock_exchange.rs, src/live/mod.rs. Nothing has been committed or pushed this session. The mock exchange work is partially done but disconnected.

### Finding 4: T53 Audit Result (from 03:25 UTC remote push)
`src/live/mock_exchange.rs` is substantial (orders/fills/positions/fee/slippage config) but is NOT wired into `src/live/bot.rs`. `mock_live_bot_v2.rs` compiles/runs but is not an end-to-end wire replacement. No cached 1m parquet exists (cache has daily/4h only). T53 goal remains: bot.rs → mock feed/executor → verify same signals as T65 harness on the same data.

### Anti-Spin Read
We are in a documentation-resetting loop. The plan gets updated, the critique gets written, and then we run another parameter sweep in the same Turtle family instead of executing the diagnostic tasks. The last 5 commits: 3 documentation, 1 genuine (freshness cooldown hyperopt), 1 genuine (T70 hyperopt but not the T70 we promised).

## Current Truth

- **Exact as-coded live bot (T65/T67):** 2.55x / daily account Sharpe 0.95 / MaxDD 28.8% / 298 trades / 1,795 Base5 days.
- **Semantic gap UNDERDIAGNOSED:** Research harness = 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades. Live bot = 2.55x / Sharpe 0.95 / MaxDD 28.8% / 298 trades. T69 semantic patch made it WORSE (1.02x). **The gap mechanism is unknown.**
- **VOL_LOOKBACK=92 unused by bot.rs** — research harness's vol-adaptive ranking absent from live execution. Strongest candidate for explaining the 70x gap.
- **Top-trade concentration risk:** Top 10 trades = 91.3% of log return. Top 5 = 57.7%.
- **Hedge overlay is inert dead code:** HEDGE_ATR_PCT (101 values, all identical) and HEDGE_SIZE_MULT (pure risk dial).
- **T68 risk governance:** 20% DD = human review trigger; 30%+ never breached; hard abandonment leaves 1.27x and misses 3 top-10 winners.
- **Live testnet:** blocked on Noah's Binance testnet keys (5+ weeks). T53 mock exchange is practical bypass.

## Next Tasks (Priority Order)

### T71: Audit Uncommitted bot.rs Changes — IMMEDIATE
**Status:** UNBUILT / CRITIQUE FINDING.
- 165+ files modified in working tree, including src/live/bot.rs, src/live/mock_exchange.rs, src/live/config.rs, src/live/executor.rs.
- The freshness-cooldown work and mock-exchange work appear to be in these files but not committed.
- Action: `git diff src/live/bot.rs` to understand what's been done in this session.
- Goal: know what progress is saved vs lost. Commit the mock exchange work if it's real.

### T53: Mock Exchange Bypass — EXECUTION BLOCKER / 5+ WEEKS OVERDUE
**Status:** PARTIALLY BUILT (mock_exchange.rs exists with substantial code, not wired to bot.rs).
- `mock_live_bot.rs` does not compile (`symbol_str` undefined + type mismatch).
- `mock_live_bot_v2.rs` compiles/runs but is not an end-to-end wire replacement for Binance WebSocket.
- No cached 1m parquet exists (cache has daily/4h only). Need either a 1m downloader or scope reduction to daily-bar mock feed.
- Goal: wire bot.rs -> mock exchange -> verify same signals as T65 harness on the same data.
- Unblocks: fills, slippage, order state, disconnect/reconnect without API credentials.

### T70: Semantic Gap Mechanism Audit — CRITICAL
**Status:** UNBUILT. The gap (research 176.79x vs live 2.54x) is NOT explained.
- **Primary hypothesis (VOL_LOOKBACK):** bot.rs does not use VL=92 in entry logic. This is the strongest candidate.
- **Hypothesis A (exit):** dual Chandelier exit vs Turtle-only produces very different trade durations.
- **Hypothesis B (sizing):** equal-size positions vs dollar-volume ranking (VL).
- **Hypothesis C (accounting):** economic mark-to-market vs realized-only equity model.
- Must include top-10 winner conditions audit: which trades produced convex winners, would any plausible filter have excluded them?
- **Do NOT run more parameter sweeps in the Turtle family.** Only test gap-relevant hypotheses.

### T61: Binance aggTrades Order-Flow Signal — TRUE ALPHA (after T53/T70)
**Status:** UNBUILT.
- Download historical Binance aggTrades; aggregate taker buy/seller-initiated imbalance into daily features.
- Genuinely new information dimension — all recent work has been price-only tuning.
- Must pass top-trade skip audit (filters cannot delete rare convex winners even if average Sharpe improves).

## Recently Closed

### T68: Drawdown Abandonment / Risk-of-Ruin Stress — DONE (2026-05-06)
- Baseline: 2.55x / MaxDD 28.8% / 298 trades.
- 20% DD triggers human review; 30%+ never breached in-sample.
- Hard abandonment: leaves 1.27x final equity, misses 3 top-10 winners (1.42x combined).
- Verdict: use 20% as review trigger, not hard abandonment.

### T67: Production Metrics Source-of-Truth Regeneration — DONE (2026-05-06)
- HALL_OF_FAME.md, daily_progress.csv, gen_hof.py now use exact-live T65/T67 only.
- Old mixed rows at daily_progress_PRE_T67_STALE.csv.
- Clean headline: exact live bot = 2.55x / Sharpe 0.95 / MaxDD 28.8% / 298 trades / 1,795 days.

### T69: Semantic Alignment Candidate — REJECTED (2026-05-05)
- 1.02x / Sharpe 0.10 / MaxDD 30.8% / 200 trades — WORSE than live bot (2.54x).
- Do NOT patch bot.rs with this candidate.

### T67: HEDGE_ATR_PCT Hyperopt — NULL (2026-05-05)
- All 101 values identical (56/63 pass, Sharpe 6.941). Dead code. No further work.

### T66: Hedge Size Mult Sweep — DONE (2026-05-05)
- Pure risk dial, not alpha. Controls position size during high-vol regimes.

### T62: Weekend Effect Filter — REJECTED (2026-05-05)
- Weekend entries are valuable, not inferior.

## Remaining Blocker
Live testnet (Noah's API keys, 5+ weeks). T53 mock exchange bypasses this.