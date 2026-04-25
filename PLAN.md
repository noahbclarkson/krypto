# PLAN.md — Krypto Research & Critique Cycle

**State: 2026-04-25 22:00 UTC. Quick fix cycle — found 2 critical bugs.**

---

## Research Loop: Effectively Closed — But We're Auditing Ourselves in Circles

The project has systematically tested all major strategy ideas. All trend-following params are frozen. The remaining untested ideas are marginal or require live data.

**What we know:**
- Turtle+Chandelier (P=7, M=2.30, EP=24, HM=12, ATR_ENTRY_MULT=0.00): 83% OOS pass, 100% Base5 pass
- Daily equity Sharpe: ~1.29 (honest number, methodology-verified)
- Edge generalizes to SPY/GLD (61% global pass, cross-market audit)
- Edge is crisis-protection, not alpha generation (bear years > bull years)
- Every entry-side filter tested: REJECTED (ATR entry, volume confirmation, correlation, chop)

**What we don't know:**
- Whether EP=24 is real or noise (only +2 windows over EP=21 in 54-window test — T3 OVERDUE)
- Whether P=7/M=2.30/HM=12 collectively hold on pre-2021 data (P=7 pre-2021 stress: 19/28 = 67.9% — marginal)
- Live execution quality (maker-fill rate, real slippage) — BLOCKED on API keys
- Whether CTREND has a viable exit mechanism (fixed-hold sweep untested)

---

## Known Production Params (VERIFIED vs src/live/config.rs 2026-04-25)

```
EP = 24              // MARGINAL: +2 windows over EP=21 in 54-window test
CHAND_PERIOD = 7     // +6.9% Sharpe vs P=11, same pass rate — plausible
CHAND_MULT = 2.30    // MARGINAL: +1 window over M=2.25 in 54-window test
ATR_ENTRY_MULT = 0.00 // CONFIRMED: definitive sweep winner, NOT marginal
HOLD_MAX = 12        // +71% Sharpe vs HM=45 — plausible but sweep used P=11 (not P=7)
ATR_PERIOD = 24      // Confirmed NULL sweep 2026-04-25 — all 26 values produced 100% pass. ATR=24 confirmed.
POSITION_CAP = 3
FRESHNESS_COOLDOWN = 0
MAX_SOL_POSITION = $50K notional
```

**⚠️ Anti-overfitting rules (established 2026-04-25):**
- Minimum win margin: ≥3 windows (5.5%) on OOS before accepting param change
- No sequential optimization on same OOS data
- Held-out validation required for marginal wins (1-2 window delta)
- Equity curve must dominate at >80% of time bars

---

## CRITICAL — Pending

### T3: EP=24 Held-Out Validation — 🔴 OVERDUE (was due 2026-04-25 AM)
- ATR_ENTRY_MULT=0.85 was REVERTED because it was optimized on the same OOS data as EP=24 — classic sequential optimization on same data
- EP=24 won by +2 windows over EP=21 (45/54 vs 43/54). At ~30% false-positive rate per window at 70% threshold: expected ~3 false winners per 96-value EP sweep. 2-window delta is noise-level.
- **Required test:** Run `regime_stress_test.rs` with EP=21 vs EP=24 on pre-2021 held-out data
  - If EP=24 ≥ EP=21 on pre-2021: keep EP=24
  - If EP=21 > EP=24 on pre-2021: revert EP=21
- **P=7 pre-2021 stress is 19/28 (67.9%)** — marginally below 70%. Combined with marginal EP=24, we have a cluster of marginal params. One clean held-out test resolves the cluster.

### T8: Live Execution Gap Monitor — ✅ BUILT, BLOCKED on live data
- Infrastructure: `examples/live_execution_audit.rs` — reads FillLog CSVs
- Synthetic test: 420 fills, avg slippage BTC -1.05bp, SOL -2.03bp, no alerts
- **Status:** Cannot be validated further without live FillLog CSV from testnet

### T6: CTREND Fixed-Hold Exit Sweep — ✅ COMPLETE (2026-04-25)
- **Signal is genuine:** Monte Carlo 0/500 shuffled beat real
- **Prior test (REJECTED):** CTREND entry + Chandelier exit → 30/54 pass (wrong mechanism — Chandelier too tight for CTREND's slower multi-horizon timing)
- **New test:** CTREND entry + fixed-hold sweep (10, 15, 21, 30, 45, 60, 90 bars)
- **Hypothesis:** CTREND multi-horizon smoothing fires SLOWER than Turtle. Fixed hold gives it room to develop. The question is whether any fixed-hold exit beats Chandelier's risk management.
- **Win condition:** Any CTREND variant >35/54 pass = viable signal family (genuinely different from Turtle)
- **Baseline:** Turtle+Chandelier = 43/54 pass (80%)
- **Why this matters:** Only untested idea producing genuinely different signal family, not parameter tuning

### T7: BTC/ETH Correlation Filter — ✅ COMPLETE (2026-04-25)
- **Result: REJECTED.** All 3 filter variants (btc_only, btc_or_eth, btc_and_eth) lose to baseline on Sharpe AND trade count.
- Delta Sharpe: -0.07 to -0.13. Trade reduction: 20-24%.
- **Conclusion:** Chandelier(P=7,M=2.30) already handles BTC-choppy regimes. Correlation filter adds no value.
- Files: `examples/turtle_correlation_filter_walkforward.rs`, `snapshots/t7_correlation_filter_report.md`

---

## BLOCKED — Waiting on Noah

### T9: Live Testnet
Noah needs Binance testnet API keys. Without this, no live paper trading.
**This is the only remaining path to new knowledge beyond T6.**

---

## Stop Doing

- **Re-running confirmed params:** ATR_PERIOD confirmed 3×. CHAND_MULT confirmed 2×. Stop.
- **Documentation-only sprints:** Last 5 commits: 4 docs/audit, 1 NULL-result cleanup. Zero feature builds.
- **Equity vanity numbers:** 246x vs 677x vs 1048x — equity is unstable across param changes. Use daily equity Sharpe (~1.29) as the stable reference.
- **Sequential optimization on same data:** We did this with EP=24 + ATR_ENTRY_MULT=0.85 simultaneously. It produced a false signal. Never again.
- **HALL_OF_FAME manual updates:** It's been stale for 5+ sessions. Build `scripts/generate_hall_of_fame.rs` instead.

---

## Quick Fixes (Do Today)

~~Fix stale print bug~~ ✅ FIXED 2026-04-25: ATR_ENTRY_MULT=0.00 constant was missing from live_turtle_chandelier.rs (compilation error). Also fixed turtle_chandelier_daily_equity.rs which used stale production params.

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| EP=24 held-out validation | CRITICAL | 🔴 OVERDUE — T3 |
| CTREND fixed-hold exit | MEDIUM | 🟡 Untested — T6 |
| Maker-fill adaptive position | LOW | 🟢 Unbuilt — needs live data |
| Live execution unknown | CRITICAL | BLOCKED on API keys |

---

## Graveyard Summary (Complete)

- All non-trend strategies: FAILED
- All regime switching: FAILED
- All entry-side filters: FAILED (ATR, volume, correlation, chop, drawdown-adaptive)
- Vol-rank overlays: FAILED
- Position scaling overlays: FAILED
- CTREND + Chandelier exit: FAILED (wrong mechanism fit)
- 4h multi-timeframe: FAILED (structural)
- Cross-market equity integration: FAILED (Sharpe -2.94 vs crypto-only)
- ATR entry filter: FAILED (definitive, 2× confirmed)
- Freshness filter: FAILED (cd=0 optimal)
- BTC/ETH correlation filter: FAILED (T7, 2026-04-25)

---

## Research Loop: Truly Closed — What Remains

**Valid new knowledge paths:**
1. T3: EP=24 held-out (running now would take 30 min — no excuse for 5-day delay)
2. T6: CTREND fixed-hold sweep (takes 2 hours — genuinely new territory)
3. Live testnet (blocked on Noah's keys)

**Everything else in strategy-ideas.md is either:**
- Already tested and rejected
- Cannot be tested without live data
- Theoretical only
