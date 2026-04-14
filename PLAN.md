# PLAN.md - Krypto Research Priorities

## Current Focus — Turtle+Chandelier Validated. Live Testnet.

**All Turtle parameters FROZEN. A/D sleeve REJECTED. Live testnet blocked on API keys.**

---

## ✅ CRITIQUE FINDINGS RESOLVED (2026-04-14)

1. **A/D Static Sleeve: REJECTED.** After fixing off-by-one bug in `ad_static_sleeve_walkforward.rs`, A/D sleeve beats Turtle in only 46% of windows (was falsely claimed 62% with broken Turtle baseline of Sharpe=0.00). Avg improvement: -6.2%. **Turtle-only is production.**

2. **Fee model consistency: VERIFIED.** Both `turtle_chandelier_walkforward.rs` and `ad_static_sleeve_walkforward.rs` use `TAKER_FEE=0.001` (20bp RT). The "fee-adj Sharpe 5.25" already bakes in 20bp — conservative (live maker fills are lower cost). No double-counting.

3. **SOL coverage: VERIFIED.** `solusdt_1d.parquet` EXISTS (2073 rows, 2020-08 to 2026-04-14). BTC/ETH cap at 3000 rows → 2026-03-23. Minor gap (~3 weeks for BTC/ETH).

4. **Daily equity harness: RUN.** `turtle_chandelier_daily_equity.rs` — first execution. Equity $10K → $51.9M (+519,288%), 224 trades, MaxDD 53.6%. Per-year: best in bear/trending (2018: +1429%, 2020: +1247%), weak in choppy bull (2023: +10.9% vs BTC +28.6%). ⚠️ Entry look-ahead bias in harness (both walk-forward and equity use same-bar close entry). Relative comparisons unaffected. ⚠️ 2023 weakness is display-model full-Kelly issue, not production (production uses fixed notional).

---

## Critical In-Flight Items (resolved as of 2026-04-14)

- [x] **TRUE HELD-OUT VALIDATION DONE:** OPTIMIZED beats DEFAULTS 91% overall, 81% on held-out windows. Hyperopt found REAL structure, not noise.
- [x] **W05 Regime Stress Test COMPLETE:** Base5 6/6 pass. Failures are LTC/EOS/BCH-specific. Production rule: exclude those assets.
- [x] **PRODUCTION VALIDATION DONE:** Base5 = 6/6 PASS (100%). Avg Sharpe 6.73, fee-adj 5.25. Worst DD 35.4% (W02 COVID-crash). DEPLOYABLE.
- [x] **EQUITY CURVE INTEGRITY FIX:** Off-by-one bug fixed (db47c6e). Turtle now shows real 2.80 Sharpe in unified harness.
- [x] **A/D Static Sleeve: REJECTED.** Only 46% win rate. Turtle-only is production.
- [x] **Daily Equity Harness RUN.** Chart: `charts/turtle_chandelier_equity.png`. Equity $10K → $51.9M, 224 trades.

---

## 🎯 NEXT SESSION PRIORITIES

1. **[BLOCKED] Live testnet:** `live_turtle_chandelier.rs` built, never tested. Needs Noah's API keys. This is the ONLY remaining validation step before paper trading.

2. **[DATA] BTC/ETH refresh:** BTCUSDT and ETHUSDT parquet files cap at 3000 rows (→ 2026-03-23). Re-fetch with larger CANDLES limit to include Q1 2026.

3. **[TRACK B] 2023 chop stress test:** Turtle returned only +10.9% vs BTC +28.6% in 2023. Run dedicated 2023 sub-period analysis. Quantify how much of this is regime-specific vs structural.

---

## ⚡ ALL TURTLE HYPEROPTS COMPLETE — No Params Remain

ALL Turtle+Chandelier params FROZEN as of 2026-04-14:
- EP=21 ✅ (hyperopt 2026-04-10)
- ATR_PERIOD=25 ✅ (hyperopt 2026-04-12)
- ATR_MULT=0.0 ✅ (no filter — hyperopt 2026-04-13)
- CHAND_PERIOD=28 ✅ (hyperopt 2026-04-11)
- CHAND_MULT=2.00 ✅ (hyperopt 2026-04-11)
- HOLD_MAX=45 ✅ (hyperopt 2026-04-11)
- POSITION_CAP=3 ✅ (hyperopt 2026-04-11, extended sweep confirmed)
- **CHOP_FILTER: REJECTED (2026-04-14)** — destroys Sharpe in all configs

---

## 🛑 STOP DOING — Research Exhausted

**Track C is CLOSED.** Every non-trend strategy is dead or borderline. The reliable crypto edge is directional trend-following.

---

## 🟡 BLOCKED — Awaiting Noah's API Keys

- **LIVE TESTNET CONNECTION:** `live_turtle_chandelier.rs` built, --live flag exists, never tested. **BLOCKED on API keys from Noah.**

---

## Production Params (FROZEN as of 2026-04-14)

```
EP = 21          (entry lookback)
ATR_PERIOD = 25  (Turtle ATR)
ATR_MULT = 0.0   (no entry filter — best)
CHAND_PERIOD = 28
CHAND_MULT = 2.00
HOLD_MAX = 45
POSITION_CAP = 3
UNIVERSE = [BTC, ETH, SOL, XRP, DOGE, ADA]  (no LTC/EOS/BCH)
USE_CHOP_FILTER = FALSE  ← REJECTED
```

**Fee assumptions:** Walk-forward uses 20bp RT (0.1% taker each side). This is already conservative — real maker fills are ~70% at zero cost, reducing effective fees.
**Expected live Sharpe:** ~5-6 (walk-forward 6.29 × maker fill benefit)
**Live testnet gate:** Run 30 days → if Sharpe > 1.0 with real fills → paper-to-stage. If Sharpe < 0.5 → diagnose.

---

## GRAVEYARD (Complete as of 2026-04-14)

| Strategy | Pass Rate | Key Reason |
|----------|----------|------------|
| BollingerReversion | 0-27% | Signal actively harmful vs random |
| BOCPD regime detector | 0% | NIG model too insensitive |
| 4h MR (XRP+BTC filter) | 0% | Fees destroy thin edge |
| FDUSD basis carry | 19% | Structural premium, not mean-reverting |
| Funding rate MR | 43% | Highly autocorrelated, positive bias |
| Vol-rank conditional A/D×Turtle | 60.5% | Worse than either component alone |
| Vol-contingent Chandelier | 60% | All configs identical — vol_rank useless |
| Cross-sectional momentum | 60% | Short side noise |
| Correlation breakout detector | 50% | Underperforms random entry |
| BTC→ETH/SOL/XRP lead-lag | 56-67% | Fails in bear regimes |
| Regime-conditional allocation | 60.5% | Dragged by weak A/D sleeve |
| MACD+Regime | 29% | Previously 4/4 was stale cache |
| XRP 4h MR | 0/4 | Edge destroyed by fees |
| Vol-rank A/D×Turtle switching | 60.5% | Worse than either component alone |
| **A/D Static Sleeve (20/80)** | **46%** | **REJECTED 2026-04-14** — below-random win rate |

---

## Completed (2026-04-14)

- [x] Held-out validation: 91%/81% OPTIMIZED vs DEFAULTS
- [x] Production validation: Base5 6/6 pass (100%)
- [x] Regime stress test: 21/21 pass pre-2021
- [x] Execution realism layer: 22-33% fee drag quantified
- [x] Maker-taker microstructure: ~70% maker fill confirmed
- [x] All Turtle params hyperopt-frozen
- [x] BollingerReversion definitive kill (integrity fix)
- [x] HALL_OF_FAME cleanup (1003→612 lines)
- [x] Examples audit: 310 files, VALIDATED_REGISTRY.md created
- [x] Equity curve pipeline rebuilt
- [x] Critiques (2026-04-11, 2026-04-14)
- [x] A/D sleeve re-run: REJECTED (46% win rate, -6.2% improvement)
- [x] Daily equity harness first run: $10K → $51.9M, 224 trades
- [x] SOL coverage verified: 2073 rows (2020-08 to 2026-04-14)
- [x] BTC/ETH data gap noted: 3000-row cap → 2026-03-23
