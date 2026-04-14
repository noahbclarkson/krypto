# PLAN.md - Krypto Research Priorities

## Current Focus — Equity Integrity Fixed. Per-Year Decomposition Done.

**All Turtle parameters FROZEN. Live testnet BLOCKED on API keys. Research CLOSED.**

---

## ✅ CRITIQUE FINDINGS RESOLVED (2026-04-14)

1. **A/D Static Sleeve: REJECTED.** After fixing off-by-one bug in `ad_static_sleeve_walkforward.rs`, A/D sleeve beats Turtle in only 46% of windows (was falsely claimed 62% with broken Turtle baseline of Sharpe=0.00). Avg improvement: -6.2%. **Turtle-only is production.**

2. **Fee model consistency: VERIFIED.** Both `turtle_chandelier_walkforward.rs` and `ad_static_sleeve_walkforward.rs` use `TAKER_FEE=0.001` (20bp RT). The "fee-adj Sharpe 5.25" already bakes in 20bp — conservative (live maker fills are lower cost). No double-counting.

3. **SOL coverage: VERIFIED.** `solusdt_1d.parquet` EXISTS (2073 rows, 2020-08 to 2026-04-14). BTC/ETH cap at 3000 rows → 2026-03-23. Minor gap (~3 weeks for BTC/ETH).

4. **Daily equity harness: FULL DATA RUN (2026-04-15).** Export cap fixed (2000→5000 bars). Full history: $10K → $67M (+670,515%), 310 trades, MaxDD 62.6%. Per-year: 2018 +1393%, 2019 +421%, 2020 +879%, 2021 +35.5% (choppy), 2022 +10.9% (BTC -45.6%), 2023 +101.9% (CORRECTED — was falsely +10.9%), 2024 +145.7%, 2025 +73.8%, 2026 -22.7% YTD.

   ⚠️ **2023 "weakness" was a data artifact.** The +10.9% figure was from stale BTC/ETH parquet (3000-row cap → 2023-03-23). Full data: +101.9% vs BTC +146.5%.

   ⚠️ **Three Sharpe numbers for Turtle:**
   - Walk-forward per-window avg: **6.29** (mean of per-window Sharpe ratios — NOT daily compounded)
   - Daily equity Sharpe (honest): **~1.0-1.3** (computed from actual daily returns on equity curve)
   - Progress chart (wrong engine): **2.80** (fixed 21-bar hold — not comparable to the above)
   - **Never report 6.29 on an equity chart.** Use ~1.0-1.3 for equity curve captions.

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

2. **[TRACK B] BTC Trend Scalar Position Sizing:** Build `btc_trend_scalar_walkforward.rs` — scale Turtle allocation based on BTC trend state (bull=100%, chop=50%, bear=25%). 9-universe sweep. Test on Base5. If pass rate stays ≥6/6 and Sharpe improves → add as production risk overlay.

3. **[TRACK A] Execution model re-audit:** Verify ~70% maker fill assumption still holds with current Binance data. Maker-taker spreads may have changed.

4. **[FIXME] Progress chart stale MACD+Regime:** MACD+Regime shows 5.21 Sharpe in progress chart but actual OOS is 2/7 pass (29%). Either re-run `macd_regime_walk_forward` for fresh equity curve, or remove from progress chart until re-validated.

5. **[TRACK B] 2021/2022 chop decomposition:** Turtle Sharpe 0.76 (2021) and 0.51 (2022) are the genuine underperformance years — not 2023. Analyze WHY Turtle lagged BTC in these chop/alternate-bull years.

---

## ✅ 2023 CHOP STRESS TEST — RESOLVED (2026-04-15)

**2023 was NOT weak — was a data artifact.**
- Previously: Turtle +10.9% vs BTC +28.6% → "weak"
- Corrected: Turtle +101.9% vs BTC +146.5% → BTC-led rally, Turtle still +101.9%
- Root cause: BTC/ETH parquet capped at 3000 rows (→ 2023-03-23), equity harness used stale BTC prices

**True problem years:**
- 2021: Sharpe 0.76 — BTC chop (+9.3%), Turtle +35.5% with 58.3% MaxDD
- 2022: Sharpe 0.51 — BTC crash (-45.6%), Turtle +10.9% (whipsawed in bear chop)
- 2026 YTD: Sharpe -5.31 — Turtle -22.7% vs BTC +12.7% (current underperformance)

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

**Honest Sharpe summary:**
- Walk-forward per-window avg: **6.29** (methodology artifact — mean of per-window Sharpe ratios; NOT daily compounded)
- Daily equity Sharpe (from full equity curve): **~1.0-1.3** ← this is the honest number
- Fee-adj walk-forward Sharpe: **~5.0** (6.29 × 0.78 maker/taker mix — inflated by W03 mega-bull)
- Progress chart (wrong engine): **2.80** (fixed 21-bar hold — incomparable)

**Expected live Sharpe:** ~1.0-2.0 range. If > 1.0 after 30 days live → proceed. If < 0.5 → diagnose.

⚠️ **The 6.29 walk-forward Sharpe is NOT directly comparable to the daily equity Sharpe (~1.0).** They are different statistical objects. The equity chart caption must use ~1.0-1.3, not 6.29.

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
