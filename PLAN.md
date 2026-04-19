# PLAN.md — Krypto Live Testnet Priority

**State: 2026-04-19 12:23 UTC. Research closed. Live testnet only. BLOCKED on API keys.**

---

## 🚨 CRITICAL — VERIFY EQUITY CURVE DATA (Highest Risk)

**This is the highest-risk issue right now. Charts in Discord may show wrong data.**

**Problem (from 2026-04-17 session):** `plot_progress.py` reads column index 4 as "Turtle" → but index 4 is `ddbudget_equity`. The real `turtle_equity` is at a different index or may have been removed from the CSV.

**What we know:**
- Current CSV has 4 columns: `day,ad_equity,small_equity,ddbudget_equity,turtle_equity`
- The `progress_equity_curves.rs` harness was supposedly fixed on Apr 17
- But the Apr 17 critique said the Python script was reading the wrong column

**Required:**
```bash
cd ~/.openclaw/workspace-krypto/krypto
cargo run --example progress_equity_curves --profile sweep
# Then manually verify: which column is which in the output CSV?
# Then check: what does plot_progress.py actually read as "turtle"?
```

**Until verified: Do NOT send equity charts to Discord.** If charts were sent in Apr 17-19 sessions, they may have been DDBudget data labeled as Turtle.

---

## 🚨 CRITICAL — HALL_OF_FAME.md Full Audit

**HALL_OF_FAME.md is wrong on critical params:**

| Parameter | HALL_OF_FAME says | Actual (live_turtle_chandelier.rs) |
|-----------|------------------|-----------------------------------|
| CHAND_PERIOD | 20 | 15 |
| CHAND_MULT | 2.15 | 1.50 |
| FRESHNESS_COOLDOWN | cd=10 (in #21 text) / cd=0 (in header) | cd=0 |

The "daily equity Sharpe ~1.34" and "6/6 pass" claims in HALL_OF_FAME were from P=20/M=2.15 runs. The current live bot uses P=15/M=1.50.

**Required:**
1. Rewrite HALL_OF_FAME.md production params section to reflect P=15/M=1.50
2. Verify all claim references are correct for the current params
3. Remove VOL_LOOKBACK from Turtle params (it's a harness-only parameter)
4. Add explicit note that walk-forward Sharpe 6.29 is NOT comparable to daily equity Sharpe

---

## 📋 LIVE TESTNET — What Needs to Happen

### What we have
- Live bot: `examples/live_turtle_chandelier.rs` ✅
- Safety interlock: `TradingMode::DryRun` default ✅
- Testnet config: `use_testnet: true` default ✅
- API keys from env: `BINANCE_API_KEY`, `BINANCE_API_SECRET` ✅

### What Noah needs to do (blocks on him)
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
Full universe, measure live Sharpe vs 1.0-1.3 expectation.

---

## 📋 POST-LIVE CONCEPTS (Build After 30-Day Live Validation)

These only matter after we have real fill data.

### S1. Live Slippage Tracker
- Track slippage per symbol per fill vs model
- SOL is the known risk: 3.7x model at $100K
- Low effort, high value

### S2. Maker-Fill Adaptive Position Sizing
- After 30 days: measure actual maker-fill % per symbol
- If <50% maker → reduce position 30%
- Low effort

### S3. Volatility Regime Dashboard
- Live ATR percentile rank display per symbol
- Color-coded: trending vs choppy
- Nice-to-have

---

## 🚫 STOP DOING

- No more walk-forward parameter optimization
- No more hyperopt passes
- No more strategy research
- No more Monte Carlo
- No more documentation loops
- No more equity chart generation until CSV is verified

---

## ⚠️ KNOWN LIVE TRADING RISKS

1. **SOL slippage exceeds model** at >$50K notional. Cap SOL position at $50K or reduce to 0.5x size.
2. **Maker fill ~63-70%** expected. If live drops below 50%, position sizing should adapt.
3. **No regime defense.** 2026 YTD -22.7% is regime whipsawing. Live Sharpe could be negative.
4. **No live stop-loss order** — exits signaled on next bar open. Gap risk exists.
5. **Equity curve may be wrong** — do not trust charts until verified.

---

*Last updated: 2026-04-19 12:23 UTC — critique session. Research closed. Live testnet only. BLOCKED on API keys.*