# PLAN.md — Krypto Research & Critique Cycle

**State: 2026-04-25 09:38 UTC. Critique cycle complete. T3 (held-out validation) 5 days overdue — EP=24 and EM=0.85 accepted without held-out confirmation. Research loop effectively closed. BLOCKED on live testnet (Noah's API keys).**

---

## Research Loop: Effectively Closed

The project has systematically tested all major strategy ideas. All trend-following params are frozen. The remaining untested ideas are marginal or require live data.

**What we know:**
- Turtle+Chandelier (P=7, M=2.25, EP=24, HM=12): 83% OOS pass, 100% Base5 pass
- Daily equity Sharpe: ~1.68 (honest number)
- Edge generalizes to SPY/GLD (61% global pass)
- Edge is crisis-protection, not alpha generation (bear years > bull years)

**What we don't know:**
- Live execution quality (maker-fill rate, real slippage)
- Whether EP=24 / EM=0.85 are real improvements or noise (T3 never run)
- Whether correlation entry filter reduces whipsaw (untested)

---

## CRITICAL — Pending

### T3: Held-Out Validation for EP=24 and ATR_ENTRY_MULT 0.85 🔴 5 DAYS OVERDUE
- **Status:** Never executed. Accepted as production defaults without held-out confirmation.
- **Problem:** EP=24 won by +2 windows (44/54 vs 42/54), EM=0.85 won by +1 window (44/54 vs 43/54). Both are within noise range for 54-window tests. The same pattern as VL=55→2.
- **Acceptance bar (must define BEFORE running):**
  - EP=24 must beat EP=21 by ≥+0.5 Sharpe OR ≥+1 window on held-out (pre-2021 data)
  - EM=0.85 must beat EM=0.90 by ≥+0.5 Sharpe OR ≥+1 window on held-out
  - If neither met → revert to EP=21 / EM=0.90
- **Run:** `held_out_validation_ep24.rs` — freeze EP and EM, test on held-out windows (pre-2021 data the walk-forward never used)
- **Risk if skipped:** More noise-level "winners" accepted as production

### T6: CTREND-Native Exit Walk-Forward 🟡 PRIORITY
- **Signal is genuine** (Monte Carlo: 0/500 shuffled beat real). But prior test paired CTREND entry with Chandelier exit — wrong mechanism fit (30/54 pass vs Turtle 43/54).
- **Hypothesis:** CTREND is slower (multi-horizon smoothing). Needs a longer-horizon exit to match its character.
- **Test:** Fixed hold sweep (10, 15, 21, 30, 45, 60 bars) × CTREND entry. Compare vs Turtle+Chandelier 43/54 baseline.
- **Win condition:** Any CTREND variant >40/54 is a viable signal family.
- **Why it matters:** Only untested idea producing genuinely different signal family, not parameter tuning.

### T7: Correlation Entry Filter for ALT Symbols 🟡
- **Hypothesis:** 2026 YTD failure (-32.8% in partial harness) may be BTC-led divergence. ALT breakouts without BTC confirmation get stopped out by tight Chandelier. BTC/ETH trend filter might reduce whipsaw.
- **Test:** Turtle+Chandelier with BTC/ETH trend confirmation filter for ALT entries.
  - No filter (baseline)
  - BTC signal required for ALT entries
  - BTC OR ETH signal (any 1-of-2)
  - BTC AND ETH signal (both required)
- **Win condition:** Filter must improve pass rate OR Sharpe without reducing trade count by >30%.
- **Risk:** Trade-starving. Every entry filter tested so far hurt pass rate.

### T8: Live Execution Gap Monitor 🟢
- **Cannot be backtested.** Build infrastructure to measure real execution quality.
- **Known risk:** SOL slippage at $50K likely 3-5bp (vs 1bp model). FillLog CSV exists but no analysis harness.
- **Build:** `examples/live_execution_audit.rs` — read FillLog CSVs, compute per-symbol: actual slippage vs model, maker-fill rate, fee paid vs expected.
- **Output:** Alert if SOL actual > 2x model → reduce SOL cap.
- **Why it matters:** The one gap between lab and live we cannot close with backtesting.

---

## BLOCKED — Waiting on Noah

### T9: Live Testnet
Noah needs Binance testnet API keys. Without this, no live paper trading.
**This is the only remaining path to new knowledge.**

---

## Stop Doing

- **Hyperopt cycling on stable params** — EP, ATR, CHAND_P, CHAND_M, HOLD_MAX, ATR_ENTRY_MULT all frozen. Stop re-running.
- **Equity vanity numbers** — stop saying "$67M", "1048.5x". Report equity Sharpe (~1.68) and pass rate (83%).
- **Re-running regime stress** — done, 67.9%, supplementary.
- **Documentation cycling** — last 5 commits: 4 docs/audit, 1 GRAVEYARD. We're auditing ourselves in circles.

---

## Production Params (FINAL — 2026-04-21, validated 2026-04-25)

```
EP=24, CHAND_PERIOD=7, CHAND_MULT=2.25, HOLD_MAX=12,
ATR_PERIOD=24, ATR_ENTRY_MULT=0.85, POSITION_CAP=3, FRESHNESS_COOLDOWN=0
```

**Source of truth: `examples/live_turtle_chandelier.rs`**

**NOTE:** EP=24 and ATR_ENTRY_MULT=0.85 are PENDING T3 validation. May revert to EP=21/EM=0.90 after held-out test.

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **T3 held-out validation** | CRITICAL | 🔴 NOT DONE — 5 days overdue |
| **Live execution unknown** | CRITICAL | BLOCKED on API keys |
| **CTREND-native exit** | MEDIUM | Untested — genuine new signal family |
| **Correlation entry filter** | MEDIUM | Untested — 2026 failure hypothesis |
| **Metric fragmentation** | MEDIUM | Ongoing — 4 Sharpe numbers in use |

---

## Graveyard Summary (Complete)

- All non-trend strategies: FAILED
- All regime switching: FAILED
- All entry-side filters: FAILED
- Vol-rank overlays: FAILED
- Position scaling overlays: FAILED
- CTREND as Turtle replacement (wrong exit): FAILED
- 4h multi-timeframe: FAILED (1/20 pass)
- Cross-market: SPY/GLD pass, QQQ marginal (58%)

---

## Research Loop: Truly Closed

Only T3 (held-out), T6 (CTREND-native exit), T7 (correlation filter), T8 (live audit) and live testnet remain as valid work.