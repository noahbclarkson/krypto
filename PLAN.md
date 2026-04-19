# PLAN.md — Krypto Live Testnet Priority

**State: 2026-04-19 17:30 UTC. Research in holding pattern. BLOCKED on API keys.**

---

## 🚨 CRITICAL — VERIFY EQUITY CURVE DATA (Highest Risk)

**Problem:** `plot_progress.py` reads column index 4 as "Turtle" → may be reading `ddbudget_equity` (not turtle_equity). Last verified: NEVER. Charts sent to Discord Apr 17-19 may show wrong data.

**Required (do this first when API keys or data question arises):**
```bash
cd ~/.openclaw/workspace-krypto/krypto
cargo run --example progress_equity_curves --profile sweep
# Then: python3 -c "
import csv
rows = list(csv.reader(open('snapshots/progress_equity_curves.csv')))
print('Columns:', rows[0])
print('Row 1:', rows[1])
print('Row -1:', rows[-1])
"
# Verify: which index = turtle_equity?
# Then check: what does plot_progress.py actually read as 'turtle'?
```

**Until verified: Do NOT send equity charts to Discord.**

---

## 🚨 CRITICAL — Daily Equity Sharpe Lock

**Problem:** HALL_OF_FAME says ~2.52. MEMORY.md says ~1.0-1.3. Both are used interchangeably. Neither has been independently verified against the same CSV pipeline that generates Discord charts.

**Required:**
```bash
# Run the verification harness
cargo run --example progress_equity_curves --profile sweep
# Then run the Python metric script
python3 gen_pr.py  # or whatever the equity Sharpe calculation script is
# Lock ONE number. Document methodology. Never mix methodologies.
```

---

## 🔲 ATR Percentile Regime Filter Walk-Forward

**Concept:** If BTC 20d ATR rank < 50th percentile of 252d history → skip new Turtle entries. Not position sizing, not stop modification. Pure entry gate.

**Why this is worth testing (despite ATR entry filter rejection):**
- Previous tests used individual-symbol ATR vs own median (trade-starving)
- This uses BTC's ATR rank as a market-wide regime signal (coarser, less noisy)
- If regime is chop (low ATR rank), skip entries regardless of which symbol triggered
- Conceptually different from all previously rejected mechanisms

**Test:** 9 universes × 7 windows. Compare: (a) Turtle with regime filter vs (b) Turtle baseline.

**Status:** Untested. Medium priority — do after equity curve verification.

---

## 🔲 CTREND + Chandelier Exit Walk-Forward

**Concept:** CTREND signal (Monte Carlo confirmed genuine, 2026-04-17) uses fixed 21-bar hold on progress chart. This exit mechanism is the same flawed class as rejected DynamicTrend. Test whether CTREND + Chandelier dual-exit improves walk-forward pass rate.

**Method:** Reuse `turtle_chandelier_walkforward.rs` harness structure, swap Turtle entry signal → CTREND signal, keep Chandelier(15,1.50) + ATR(24,2.0) dual exit.

**If CTREND + Chandelier pass rate > CTREND fixed hold:** CTREND becomes a second validated signal family and potential production sleeve.

**Status:** Untested. Low priority — research is closed, this is a curiosity.

---

## 📋 LIVE TESTNET — What Needs to Happen

### What we have
- Live bot: `examples/live_turtle_chandelier.rs` ✅
- Safety interlock: `TradingMode::DryRun` default ✅
- Testnet config: `use_testnet: true` default ✅
- API keys from env: `BINANCE_API_KEY`, `BINANCE_API_SECRET` ✅

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

### S1. Live Slippage Tracker ✅ BUILD NOW (before live)
- Track slippage per fill per symbol vs model
- SOL is known risk: 3.7x model at $100K
- Even in dry-run: log expected vs actual fill price
- Low effort, high value for post-live analysis

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

- No more walk-forward parameter optimization on frozen params
- No more ATR_MULT, ATR_PERIOD, CHAND_MULT sweeps (all confirmed, redundant)
- No more equity chart generation until CSV is verified
- No more documentation loops — fix the equity CSV first
- No more Monte Carlo on CTREND (signal is confirmed real)

---

## ⚠️ KNOWN LIVE TRADING RISKS

1. **2026 YTD regime whipsawing** — worst year on record (-22.7%, Sharpe -5.31). Strategy unproven in current market conditions.
2. **SOL slippage** — exceeds model at >$50K notional. Cap SOL at $50K or reduce to 0.5x.
3. **Maker fill ~63-70%** expected. If live drops below 50%, position sizing should adapt.
4. **No live stop-loss order** — exits signaled on next bar open. Gap risk exists.
5. **Equity curve may be wrong** — do not trust charts until verified.

---

## Blind Spots Confirmed This Session

| Blind Spot | Severity | Notes |
|-----------|----------|-------|
| 2026 YTD regime whipsawing | HIGH | -22.7%, Sharpe -5.31 — our worst year |
| Equity CSV unverified | HIGH | Column mapping never confirmed |
| CTREND exit = fixed hold | MEDIUM | Same flaw class as rejected DynamicTrend |
| Daily equity Sharpe inconsistent | MEDIUM | 1.04 vs 2.52 — pick one, lock it |
| Hyperopt redundancy loop | LOW | ATR swept 3x, HM swept 2x — all confirm same values |

---

*Last updated: 2026-04-19 17:30 UTC — critique session. Research in holding pattern. Live testnet blocked on API keys.*
