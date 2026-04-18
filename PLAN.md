# PLAN.md — Krypto Live Testnet Readiness

## ⚡ CRITIQUE FINDINGS (2026-04-18 20:05 UTC — Strategy Research & Critique)

### Last 8 Commits Assessment
```
f3d9135a chore:  TradingMode interlock + testnet PLAN              ← docs/infra
1905c2c4 feat:   TradingMode safety interlock                      ← real (minimal)
758a12f4 docs:   hyperopt session update (memory hygiene)          ← docs
8153d5ce feat:   TURTLE_ATR_MULT extensive sweep + ATR period     ← redundant hyperopt
25816f95 feat:   head-to-head benchmark + MACD + parquet          ← real (genuine)
3ecd1a58 docs:   critique and plan update                          ← docs
fbcb7847 chore:  daily progress tracking                           ← docs
0b46f45f docs:   research loop closed                              ← docs
```
**2/8 genuine discoveries (head-to-head benchmark, parquet refresh). ATR_MULT/ATR_PERIOD re-sweeps are redundant — both params already frozen since 2026-04-12 and 2026-04-16 respectively.**

### ATR_MULT and ATR_PERIOD Re-sweep: Redundant
- **ATR_MULT=2.0** — already frozen since 2026-04-12. The 9-value sweep is identical to the original and confirms M=2.0. Not new knowledge.
- **ATR_PERIOD=24** — already frozen since 2026-04-16. The 5-100 step=5 sweep confirms ATR=24. ATR=95 was already suspected as cap artifact. Not new knowledge.
- **Pattern:** More sweeps on frozen params → illusion of progress, real risk of aggregate overfitting.

### All Strategy Ideas Exhausted
| # | Idea | Status |
|---|------|--------|
| 18 | DD-adaptive EP tightening | 🪦 REJECTED — ATR entry filter proved ANY entry tightening hurts |
| 21 | Freshness filter cd=N | ✅ DONE — cd=10 implemented in live bot |

### Reports CSV Stale Data: Known Limitation
- `reports/daily_progress.csv` contains BollingerRev DOGE Sharpe 19.01 (0/288 OOS) and other prototype artifacts
- Not fixable without report versioning system — noted as known limitation

### Freshness Filter cd=10: Modest Improvement, Possible Overfit
- cd=10 won 31-value sweep (+0.4pp pass rate vs cd=3)
- Improvement is within noise. Live testnet is the only validation.
- Risk: cd=10 might be overfit to 9-universe aggregate; cd=0 might perform identically in live

### 3 Most Promising Unbuilt Ideas

**1. Maker-Fill Adaptive Execution — NOT TESTABLE until live**
- Detect low maker-fill probability → switch to aggressive execution
- Mechanistically sound but needs live order flow data
- **Status: WAIT for API keys**

**2. Regime-Contingent Cooldown — LOW PRIORITY**
- cd = f(ATR_percentile_rank)
- Every regime-contingent idea has failed (USDT hedge, BTC scalar, chop filter, DD trigger)
- **Status: DEFER until live data available**

**3. No genuinely new ideas remain.** Strategy-ideas.md is exhausted.

### Metrics: TRUSTWORTHY ✅
- CTREND/Turtle Monte Carlo: 0/500 and 0/100 shuffled beat real — edge GENUINE
- Freshness filter cd=10: small but documented improvement
- Equity Sharpe ~1.0: honestly labeled

### Biggest Blind Spots

**1. No regime defense in live bot (structural, unfixable)**
- 2026 YTD: Turtle -22.7% vs BTC +12.7%
- Every overlay tested and failed: USDT hedge, BTC scalar, chop filter, drawdown trigger
- **Honest conclusion:** We don't know how to defend against chop. Stay in the strategy and accept underperformance in non-trending regimes.

**2. Maker fill gap is biggest unmeasured variable**
- 70% maker assumption (~63% actual) never tested live
- Live vs backtest drift unknown
- **Only live testnet answers this. Blocked on API keys.**

**3. cd=10 might be noise-range overfit**
- Improvement over cd=0 is +0.4pp pass rate — within noise
- If overfit, cd=0 (simpler) is equivalent or better

---

## TOP 3 PRIORITY EXECUTION TASKS

**T1 — Live testnet (BLOCKED on Noah's Binance testnet API keys):**
- `live_turtle_chandelier.rs` built, `TradingMode` interlock in place — ✅ READY
- Only blocker: Noah's Binance testnet API keys
- Once keys arrive: run `--live` (testnet shadow mode) for 48h to verify signals
- Then: small real testnet trades (5-10, 1 symbol) to validate execution
- Then: 30-day full universe testnet → compare live vs backtest Sharpe (~1.0 expected)
- Metric to track: `live_vs_backtest_drift = (live_sharpe - 1.2_ref) / 1.2_ref`. Acceptable: -40% to +20%

**T2 — STOP ALL hyperopt, documentation, and research loops:**
- ATR_MULT=2.0 ✅ confirmed (frozen 2026-04-12)
- ATR_PERIOD=24 ✅ confirmed (frozen 2026-04-16)
- cd=10 ✅ implemented
- All Turtle+Chandelier params FROZEN. No more sweeps.
- Any future hyperopt must be justified by a specific live trading failure, not curiosity

**T3 — Document live trading risks formally (low effort, high value):**
- Create `snapshots/live_trading_risks.md` with: SOL slippage cap ($50K notional), maker fill gap (63% vs 70%), no regime defense (2026 YTD -22.7% is cost of staying), all parameters frozen, expected live Sharpe ~1.0
- One-page reference for Noah before testnet launch

---

## ✅ PREVIOUS PRIORITIES RESOLVED

**Freshness filter #21 CLOSED ✅**
- cd=10 implemented in `src/live/bot.rs`
- Live bot: production-ready with cd=10

**VOL_LOOKBACK removed from Turtle+Chandelier ✅**
- Parameter belongs to DDBudget only
- HALL_OF_FAME.md stale entry removed from Turtle params

**ATR_MULT and ATR_PERIOD fully validated ✅**
- Re-sweep (commit 8153d5ce) confirms both params — no change needed, no further sweeps warranted

**Research loop CLOSED ✅**
- All strategy ideas tested or rejected
- Project in live testnet readiness state

---

## Production Params (FROZEN as of 2026-04-18)

```
EP = 21             (entry lookback)
ATR_PERIOD = 24     (Turtle ATR exit — fine hyperopt 2026-04-16)
TURTLE_ATR_MULT = 2.0  (Turtle ATR stop)
ATR_MULT = 0.0      (no entry filter — definitive, 2026-04-13)
CHAND_PERIOD = 20   (Chandelier exit)
CHAND_MULT = 2.15   (saturation plateau confirmed)
HOLD_MAX = 45
POSITION_CAP = 3
FRESHNESS_COOLDOWN = 10  (bars after exit before same-symbol re-entry)
UNIVERSE = [BTC, ETH, SOL, XRP, DOGE]  (NoDOGE — ADA removed)
USE_CHOP_FILTER = FALSE  ← REJECTED
MAX_SOL_POSITION = $50K notional  ← due to SOL slippage risk
```

**Honest Sharpe summary:**
- Walk-forward per-window avg: **6.29** (NOT directly comparable — methodology artifact)
- Daily equity Sharpe (NoDOGE): **~1.0** ← use this on equity chart captions
- Progress chart (NoDOGE): **~2.5** (Chandelier dual-exit)
- **Never report 6.29 on an equity chart. Use ~1.0.**
- **Expected live Sharpe:** ~1.0-2.0. Acceptable drift: -40% to +20%.

---

## GRAVEYARD (Complete as of 2026-04-18)

| Strategy | Status | Key Reason |
|----------|--------|------------|
| BollingerReversion | GRAVEYARD | Signal 0% OOS pass, worse than random |
| BOCPD regime detector | GRAVEYARD | 0% breaks detected |
| FDUSD basis carry | GRAVEYARD | Structural premium, autocorrelation 0.88 |
| Funding rate MR | GRAVEYARD | 43% pass, highly autocorrelated |
| Cross-sectional momentum | GRAVEYARD | 60% pass, short side noise |
| Vol-contingent Chandelier | GRAVEYARD | All configs identical |
| Vol-rank A/D×Turtle switching | GRAVEYARD | Worse than either component alone |
| MACD+Regime | GRAVEYARD | 2/7 OOS pass |
| 4h MR | GRAVEYARD | 0/4 pass, fees destroy edge |
| Regime-conditional allocation | GRAVEYARD | 60.5% pass, dragged by weak A/D |
| BTC Trend Scalar | GRAVEYARD | 0/8 configs beat baseline |
| A/D Static Sleeve | GRAVEYARD | 47% pass |
| ATR entry filter | GRAVEYARD | Any non-zero ATR filter hurts pass rate |
| DD-adaptive EP tightening | GRAVEYARD | Same mechanism as ATR entry filter |
| DynamicTrend+Chandelier | GRAVEYARD | Turtle wins 21/24 windows |
| Cross-Market Equity Portfolio | GRAVEYARD | Combined worse than crypto-only |
| SOL Dollar-Sized Re-test | GRAVEYARD | Cap never activates in walk-forward |

---

## Live Testnet Launch Plan

### Phase 1: Dry-Run Verification (No API keys needed)
```bash
cargo run --example live_turtle_chandelier --profile sweep
# Verify: compiles, loads parquet data, simulates signals, prints equity
```
**Status:** ✅ DONE. Build verified, dry-run mode works.

### Phase 2: Testnet Shadow Mode (API keys required)
```bash
export BINANCE_TESTNET_API_KEY=<your_key>
export BINANCE_TESTNET_API_SECRET=<your_secret>
cargo run --example live_turtle_chandelier --profile sweep -- --live
```
- **Effect:** Real market data, simulated orders (no real funds used on testnet)
- **Duration:** 48h minimum to verify signal quality vs backtest
- **Success criteria:** Signals fire at expected rate (~6-7% of bars per symbol)

### Phase 3: Small Real Testnet Trades (1-2 weeks in)
```bash
cargo run --example live_turtle_chandelier --profile sweep -- --live --small
```
- **Effect:** Real testnet orders with minimal size ($50-100/notional)
- **Purpose:** Validate maker fill rate, slippage vs model, order execution latency
- **Success criteria:** Actual fees match model (63-70% maker), slippage within 2bp

### Phase 4: 30-Day Full Universe Run
- All 5 symbols, full position sizing
- Compare live equity Sharpe vs backtest ~1.0 reference
- **Decision:** If Sharpe > 1.0 → proceed toward production. If < 0.5 → diagnose.

---

## Binance Testnet Readiness Checklist (Noah)

1. **Create Binance testnet account:** https://testnet.binance.vision/
2. **Generate API key + secret** (read permissions minimum, no withdrawal)
3. **Faucet testnet funds:** https://testnet.binance.vision/fundatory/ — request BTC, ETH, SOL, XRP, DOGE
4. **Set env vars:**
   ```bash
   export BINANCE_TESTNET_API_KEY=<your_key>
   export BINANCE_TESTNET_API_SECRET=<your_secret>
   ```
5. **Verify connection:**
   ```bash
   cargo run --example live_turtle_chandelier --profile sweep -- --live
   # Should print "TESTNET MODE" banner + start consuming live data
   ```

---

## What's Already Safe

✅ Signal logic — Monte Carlo validated (0/500 shuffled beat real)
✅ Freshness filter cd=10 — implemented in live bot
✅ Dual Chandelier(20,2.15)+Turtle_ATR(24,2.0) exit
✅ Position cap = 3 symbols
✅ `TradingMode` safety interlock (Production mode blocked without `--prod`)
✅ Dry-run mode (simulated orders, no API keys needed)
✅ 20bp conservative fee model (real fees ~15bp)

## Known Live Trading Risks

⚠️ **SOL slippage:** Modeled 1bp, actual ~3.7bp at $100K. Cap SOL at $50K notional.
⚠️ **Maker fill:** 63% actual vs 70% model assumption → slightly higher fee drag
⚠️ **No regime defense:** Strategy underperforms in extended chop (2026 YTD: -22.7% vs BTC +12.7%)
⚠️ **Maker vs taker slippage on exit:** Chandelier exit mostly taker (~25% maker fill)
⚠️ **2026 YTD is regime-inherent whipsawing** — not a strategy failure

---

## STOP DOING — Absolute Red Lines

🚫 **No more hyperopt on Turtle+Chandelier params** — all frozen
🚫 **No more research documentation loops** — research is closed
🚫 **No pushing to main** — always `v2-rewrite`
🚫 **No live trading with real funds** — testnet first, then Noah's explicit approval
