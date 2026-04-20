# PLAN.md — Krypto Live Testnet Priority

**State: 2026-04-20 15:05 UTC. Paired pre-2021 stress test PASSED. P=11/M=2.25 = 21/21, ΔSH=-0.02 vs P=28/M=2.0. HALL_OF_FAME and PLAN docs updated. Research CLOSED. Live testnet is the only path forward.**

---

## ✅ RESOLVED — CHAND_P=11/M=2.25 Pre-2021 Paired Test PASSED

**Paired pre-2021 stress test** (`examples/regime_stress_paired.rs`, commit `d81926b5`):
- OLD P=28/M=2.0 (prior validated): **21/21 pass**, avg Sharpe **0.79**
- NEW P=11/M=2.25 (current production): **21/21 pass**, avg Sharpe **0.77**
- **ΔSH = -0.02** (essentially equivalent — new params are NOT overfitting)
- By phase: P1-2020 Δ+0.02, P2-2021 Δ-0.05, P3-2019 Δ-0.01
- **Docs updated:** HALL_OF_FAME.md now reflects P=11/M=2.25 (was P=5/M=3.00 — 2 hyperopt rounds stale)

---

## 📋 LIVE TESTNET — What Needs to Happen

### What we have (all ✅)
- Live bot: `examples/live_turtle_chandelier.rs` ✅ (synced to P=11/M=2.25)
- Safety interlock: `TradingMode::DryRun` default ✅
- Testnet config: `use_testnet: true` default ✅
- API keys from env: `BINANCE_API_KEY`, `BINANCE_API_SECRET` ✅
- Slippage Tracker: `FillLog` struct, CSV logging, `log_fill()` on all orders ✅
- Pre-2021 validation: P=11/M=2.25 passes 21/21, ΔSH=-0.02 ✅
- Walk-forward: 43/54 (79.6%) global, 6/6 (100%) Base5 ✅
- Daily equity Sharpe: ~1.04 (honest) ✅

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

## ⚠️ KNOWN LIVE TRADING RISKS

1. **2026 YTD regime whipsawing** — worst year on record (-22.7%, Sharpe -5.31). Strategy unproven in current market conditions.
2. **SOL slippage** — exceeds model at >$50K notional. Cap SOL at $50K or reduce to 0.5x.
3. **Maker fill ~63-70%** expected. If live drops below 50%, position sizing should adapt.
4. **No live stop-loss order** — exits signaled on next bar open. Gap risk exists.
5. **Slippage Tracker** — SOL slippage is the primary risk to measure first.

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

- No more walk-forward parameter optimization — params are frozen
- No more ATR_MULT, ATR_PERIOD, CHAND_P/M sweeps — P=11/M=2.25 confirmed by pre-2021 paired test
- No more ATR_ENTRY_MULT — 0.00 is production
- No more ATR_EMA smoothing — raw ATR confirmed optimal
- No more FRESHNESS_COOLDOWN sweeps — 0 is optimal
- No more HOLD_MAX sweeps — 45 is safe plateau
- No more equity chart posting without running the harness and verifying column mapping
- No more documentation loops — docs are now synced to config.rs
- No more research on non-trend strategies — all GRAVEYARD'd

---

## Blind Spots Confirmed

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **2026 YTD regime unexplained** | CRITICAL | Open — strategy unproven in current conditions |
| **Parameter drift in docs** | HIGH | **RESOLVED 2026-04-20** — HALL_OF_FAME + PLAN now match config.rs |
| **Equity curve column mapping** | HIGH | **RESOLVED** — re-run harness before next Discord post |
| **Multi-timeframe (4h)** | MEDIUM | Open — documented, not coded |
| **CTREND + Chandelier exit** | MEDIUM | Open — curiosity only, live testnet is priority |

---

## Production Params (P=11/M=2.25 — Pre-2021 Paired Test PASSED, FINAL)

```
EP = 21, ATR_PERIOD = 24, ATR_MULT = 0.0
CHAND_PERIOD = 11, CHAND_MULT = 2.25
ATR_ENTRY_MULT = 0.00
HOLD_MAX = 45, POSITION_CAP = 3, FRESHNESS_COOLDOWN = 0
MAX_SOL_POSITION = $50K notional
```

*Last updated: 2026-04-20 15:05 UTC — paired pre-2021 test 21/21 pass, ΔSH=-0.02. HALL_OF_FAME and PLAN synced to config.rs.*

## Equity Numbers — QUOTE ONLY VERIFIED NUMBERS

| Value | Source | Date | Status |
|-------|--------|------|--------|
| ~1.0-1.3 | Daily equity Sharpe | 2026-04-16 | HONEST |
| 43/54 (79.6%) | 9-universe walk-forward | 2026-04-20 | VERIFIED |
| 6/6 (100%) | Base5 walk-forward | 2026-04-20 | VERIFIED |
| 21/21 | Pre-2021 paired test (P=11/M=2.25) | 2026-04-20 | VERIFIED |
| 1048.5x | P=5/M=3.00 full-history | 2026-04-20 | STALE — not re-run for P=11/M=2.25 |
| $10K→$67M | P=5/M=3.00 full-history | 2026-04-15 | STALE |
