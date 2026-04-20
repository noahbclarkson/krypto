# PLAN.md — Krypto Live Testnet Priority

**State: 2026-04-20 14:07 UTC. ATR_ENTRY_MULT=0.20 REVERTED (overfits pre-2021). P=5/M=3.00 pre-2021 stress test PASS (21/21). Production params FINAL.**

---

## 🚨 CRITICAL — ATR_ENTRY_MULT=0.20 REVERTED (2026-04-20 14:07 UTC)

**⚠️ REVERTED.** Pre-2021 held-out stress test (examples/regime_stress_p5m3.rs):
- ATR_ENTRY=0.20: 21/21 pass, Sharpe **0.32**, log return +1438%
- ATR_ENTRY=0.00: 21/21 pass, Sharpe **0.36**, log return +1650%

The ATR_ENTRY=0.20 filter (added 2026-04-20 AM, +30.7% post-2021 OOS) is OVERFITTING to post-2021 microstructure. Reverted to 0.00 in config.rs. Live bot now matches walk-forward harness exactly.

**Pre-2021 Stress Test RESULT: P=5/M=3.00 CONFIRMED ROBUST**
- P3-2019 (Bear): 4/4 pass ✅
- P1-2020 (COVID+Bull): 7/7 pass ✅
- P2-2021 (Mega-Bull): 10/10 pass ✅
- **Global: 21/21 (100%)** — the Chandelier(P=5, M=3.00) is genuinely regime-robust

---

## ✅ RESOLVED — Daily Equity Sharpe Locked

**Honest equity Sharpe = 1.29 (daily compounding, CHAND_P=5, CHAND_M=3.00).**
Turtle final equity: 1048.5x (was 724.4x at P=15/M=2.25 — +45% improvement from tighter stop).
Walk-forward avg Sharpe (4.91) is methodology-inflated — NOT directly comparable to equity Sharpe.
HALL_OF_FAME updated: equity Sharpe locked at 1.29. Report this number only.

---

## 📋 LIVE TESTNET — What Needs to Happen

### What we have
- Live bot: `examples/live_turtle_chandelier.rs` ✅ (now synced to P=5/M=3.00)
- Safety interlock: `TradingMode::DryRun` default ✅
- Testnet config: `use_testnet: true` default ✅
- API keys from env: `BINANCE_API_KEY`, `BINANCE_API_SECRET` ✅
- Slippage Tracker: `FillLog` struct, CSV logging, `log_fill()` on all orders ✅ (commit 8066cab3)

### What Noah needs to do (blocks on us)
1. Create testnet account: https://testnet.binancefuture.com/
2. Faucet test USDT + test BTC/ETH/SOL/XRP/DOGE
3. Give Kira the API key + secret (or set env vars on VPS)

### Phase 1: Shadow Run (48h)
```bash
BINANCE_API_KEY=xxx BINANCE_API_SECRET=yyy \
  cargo run --example live_turtle_chandelier --profile sweep -- --live
```
Verify: signals fire on live WebSocket, no crashes, `[DRY RUN]` prefix.

### Phase 2: Small Real Testnet Trade (5-10 trades, BTC only)
Change `dry_run: false` in example, run 1 symbol.

### Phase 3: Full 5-Symbol 30-Day Run
Full universe, measure live Sharpe vs 1.0-1.3 honest expectation.

---

## 📋 POST-LIVE CONCEPTS (Build After 30-Day Live Validation)

### S1. Maker-Fill Adaptive Position Sizing
- After 30 days: measure actual maker-fill % per symbol
- If <50% maker → reduce position 30%
- Low effort

### S2. Volatility Regime Dashboard
- Live ATR percentile rank display per symbol
- Color-coded: trending vs choppy
- Nice-to-have

---

## 🚫 STOP DOING

- No more walk-forward parameter optimization on frozen params
- No more ATR_MULT, ATR_PERIOD sweeps (confirmed redundant, 3+ times each)
- No more CHAND_P/M sweeps — P=5/M=3.00 is the joint optimum, confirmed 9-universe WF
- No more documentation loops — params are synced and verified
- No more Monte Carlo on CTREND (signal confirmed real)
- No more equity chart loop — charts now correct (1048.5x with P=5/M=3.00)

---

## ⚠️ KNOWN LIVE TRADING RISKS

1. **2026 YTD regime whipsawing** — worst year on record (-22.7%, Sharpe -5.31). Strategy unproven in current market conditions. No explanation found.
2. **SOL slippage** — exceeds model at >$50K notional. Cap SOL at $50K or reduce to 0.5x.
3. **Maker fill ~63-70%** expected. If live drops below 50%, position sizing should adapt.
4. **No live stop-loss order** — exits signaled on next bar open. Gap risk exists.
5. **Slippage Tracker** — built and ready. SOL slippage is the primary risk to measure first.

---

## Blind Spots Confirmed

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **2026 YTD regime unexplained** | CRITICAL | Open — strategy unproven in current conditions |
| **ATR_ENTRY_MULT overfitting** | HIGH | **RESOLVED 2026-04-20** — Reverted to 0.00, pre-2021 stress confirmed |
| **Pre-2021 stress test for P=5/M=3.00** | HIGH | **RESOLVED 2026-04-20** — 21/21 pass, P=5/M=3.00 is robust |
| **Parameter churn** | HIGH | Resolved — params now stable |
| **live_turtle_chandelier.rs stale** | HIGH | Resolved |
| **progress_equity_curves.rs stale** | HIGH | Resolved |
| **CTREND fixed hold** | MEDIUM | Open — signal real, exit mechanism unvalidated |
| **Multi-timeframe (4h)** | MEDIUM | Open — documented, not coded |

---

## Production Params (P=5/M=3.00 — Pre-2021 Stress Test PASSED, FINAL)

```
EP = 21, ATR_PERIOD = 24, ATR_MULT = 0.0
CHAND_PERIOD = 5, CHAND_MULT = 3.00
ATR_ENTRY_MULT = 0.00  ← REVERTED (was 0.20, overfits pre-2021)
HOLD_MAX = 45, POSITION_CAP = 3, FRESHNESS_COOLDOWN = 0
MAX_SOL_POSITION = $50K notional
```

*Last updated: 2026-04-20 14:07 UTC — pre-2021 stress test 21/21 pass, ATR_ENTRY reverted.*