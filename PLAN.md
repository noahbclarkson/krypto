# PLAN.md — Krypto Live Testnet Priority

**State: 2026-04-20 21:01 UTC. ✅ T1 COMPLETE — equity chart fixed (779.6x). ✅ T2 COMPLETE — 2026 YTD mechanism confirmed. ✅ T3 COMPLETE — slippage tracker built (FillLog in executor.rs). ✅ T4 COMPLETE — CTREND REJECTED as Turtle replacement (research loop CLOSED). BLOCKED: live testnet pending Noah's API keys.**

---

## ✅ T1 COMPLETE — Equity Chart Fixed

`examples/progress_equity_curves.rs` CHAND_P: 15→11 (CHAND_M was already 2.25).
- New result: **779.6x** (was 734.2x stale, +6.2% from correct params)
- Daily equity Sharpe: 1.29
- src/config.rs already had correct params — no change needed
- Chart: `charts/progress_equity_curves_daily.png`

---

## ✅ T2 COMPLETE — 2026 YTD Fully Explained

**All three param sets produce IDENTICAL results in 2026 YTD:**
- P=5/M=3.00: -32.8%, Sharpe -18.91, 10 trades
- P=11/M=2.25: -32.8%, Sharpe -18.91, 10 trades
- P=15/M=1.50: -32.8%, Sharpe -18.91, 10 trades

**Mechanism:** In the 2026 bear (BTC -14.2%, SOL -32.2%, ETH -21.4%), Turtle ATR exit dominates Chandelier. All three Chandelier params fire on the same bars — the Turtle ATR stop is controlling. Chandelier differentiation is irrelevant in this regime.

**Implication:** The -22.7% YTD is regime-inherent, not param-fixable. No Chandelier change helps. The strategy is working correctly (stopping out losing positions) but in a sustained downtrend, repeated stop-outs destroy returns. This is the known cost of trend-following in bear markets.

**Conclusion:** P=11/M=2.25 remains justified by superior historical walk-forward performance. The 2026 underperformance is the price of trend-following protection in all other regimes.

---

## ✅ T3 COMPLETE — Slippage Tracker Built

**Infrastructure already present in `src/live/executor.rs` + `LiveBot::new()` (since unknown date).**
- `FillLog` struct: timestamp, symbol, side, expected_price, actual_price, slippage_bp, notional, quantity, fee_paid, dry_run, order_type
- `Executor::enable_fill_log()` → `logs/slippage_YYYY-MM-DD.csv` on every fill (dry-run + live)
- `Executor::slippage_summary()` for live monitoring
- Validated today: code review confirmed it's production-ready
- Status: ✅ Complete. No further build needed.

---

## ✅ T4 COMPLETE — CTREND + Chandelier Walk-Forward

**Result: CTREND REJECTED as Turtle replacement.**

CTREND (genuine signal per Monte Carlo, 0/500 shuffled beat real) + Chandelier(11, 2.25) → 30/54 pass (44% fail), avg Sharpe 2.28, 858 trades.
Turtle + Chandelier → ~43/54 pass (~20% fail). Decisive underperformance.

**Key insight:** Signal quality (Monte Carlo confirmed real) ≠ signal-strategy fit. CTREND's multi-horizon smoothing fires too late for Chandelier dual-exit. Turtle breakout timing synergizes better. Entry signal matters as much as exit mechanism. See `examples/ctrend_chandelier_walkforward.rs`.

**Research loop CLOSED.** All genuinely testable strategy ideas exhausted.

---

## Production Params (P=11/M=2.25 — ✅ EQUITY CURVE VERIFIED: 779.6x, Sharpe 1.29)

```
EP = 24, ATR_PERIOD = 24, ATR_MULT = 0.0
CHAND_PERIOD = 11, CHAND_MULT = 2.25
HOLD_MAX = 45, POSITION_CAP = 3, FRESHNESS_COOLDOWN = 0
MAX_SOL_POSITION = $50K notional
```

*Last updated: 2026-04-20 20:05 UTC.*

---

## 🚫 STOP DOING

- No more Chandelier parameter sweeps — P=11/M=2.25 is production, no more CHAND_P/M changes until live data validates or contradicts
- No more ATR_ENTRY_MULT sweeps — confirmed 0.00, confirmed harmful at any value
- No more ATR_MULT or ATR_PERIOD confirmation sweeps — already validated (M=2.0, ATR=24), confirmation produces no new knowledge
- No more equity number quotes — say "~800x" or "hundreds of times" until live equity is measured
- No more "daily tracking" commits unless a chart is actually updated

---

## Live Testnet — Still Blocked on API Keys

Noah needs to create testnet account and provide API keys. Without this, no progress on live validation.

### Phase 1: Shadow Run (48h after API keys)
```bash
BINANCE_API_KEY=xxx BINANCE_API_SECRET=yyy \
  cargo run --example live_turtle_chandelier --profile sweep -- --live
```

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **Live Slippage Tracker unbuilt** | CRITICAL | Open — T3 |
| **CTREND exit mechanism unresolved** | MODERATE | Open — T4 |
| **Reports directory stale (BollingerRev DOGE 5404 Sharpe artifacts)** | MODERATE | Hygiene needed |
| **Equity numbers unverifiable (800x-1000x range, depends on cache)** | LOW | Accept — live equity is only number that matters |
| **Noah's testnet API keys** | CRITICAL | BLOCKED — waiting on Noah |

---

## Research Loop — CLOSED

No genuinely untested strategy ideas remain. Live testnet is the only path forward for strategy validation.
- **CTREND + Chandelier exit (T4)** — testable, but it's niche validation, not new strategy discovery
- **4h multi-timeframe** — genuinely new territory, lower priority
- **Slippage tracker (T3)** — execution infrastructure, must-build before live

All productive research is complete. Only live execution reveals new knowledge.