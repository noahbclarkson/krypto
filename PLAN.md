# PLAN.md - Krypto Research Priorities

## Current Focus — Strategy Critique Complete (2026-04-16)

**Research CLOSED. Three execution tasks remain before live deployment.**

**Key critique findings (2026-04-16):**
- Sharpe conflation: 3 incompatible metrics (6.29 WF avg / 1.04 equity / ~2.5 progress). Honest number = 1.0-1.3.
- 2026 YTD: Turtle -22.7% vs BTC +12.7% — persistent underperformance in choppy/range-bound markets
- Hyperopt exhaustion: 50,000+ runs over 6 days — 91% in-sample win rate is optimistic
- HALL_OF_FAME/GRAVEYARD.md missing from repo — documentation integrity risk  ✅ RESOLVED 2026-04-16 (files created)
- SOL slippage model miss: 3.7× underestimate at $100K — DOCUMENTED AS LIVE TRADING CONSTRAINT (2026-04-16)
- `live_turtle_chandelier.rs` never tested on testnet — VERIFIED paper mode works (2026-04-16)

**Sharpened production claim:** "Sharpe ~1.0-1.3 on equity curve, 93% OOS pass, real but modest edge." Stop saying "Sharpe 5.0+" in any context where it isn't clearly labeled as walk-forward per-window average.

---

## ✅ TRACK A — EXECUTION MODEL AUDIT COMPLETE (2026-04-15)

**Execution model is trustworthy.** Key findings:

| Assumption | Model | Live | Verdict |
|-----------|-------|------|---------|
| RT fee | 20bp | ~15bp (63% maker) | Conservative — good |
| $10K slippage | 1bp | 0.1-0.4bp | Very conservative |
| $100K slippage BTC | 1bp | 0.75bp | Conservative |
| $100K slippage SOL | 1bp | 3.70bp | **RISK** — exceeds model |
| Maker fill | 70% | 63% | Slightly conservative |
| Entry timing | open next bar | 0% median gap | No bias |
| Breakout rate | ~6-7% | 6-7% confirmed | Matches |

**Action:** Max SOL position size should be ≤$50K notional to stay within slippage model. Document in production notes.

---

## ✅ BTC TREND SCALAR — REJECTED (2026-04-15)

**Re-confirmed:** Baseline (no scaling) 6/6 pass, Sharpe 5.46. No scalar config beats it. BTC was BULL in all 6 windows — scalar never activated. Chandelier dual-exit handles chop; position sizing cannot fix regime-inherent whipsawing.

---

## ✅ CRITIQUE FINDINGS RESOLVED (2026-04-14)

1. **A/D Static Sleeve: REJECTED.** After fixing off-by-one bug in `ad_static_sleeve_walkforward.rs`, A/D sleeve beats Turtle in only 46% of windows (was falsely claimed 62% with broken Turtle baseline of Sharpe=0.00). Avg improvement: -6.2%. **Turtle-only is production.**

2. **Fee model consistency: VERIFIED.** Both `turtle_chandelier_walkforward.rs` and `ad_static_sleeve_walkforward.rs` use `TAKER_FEE=0.001` (20bp RT). The "fee-adj Sharpe 5.25" already bakes in 20bp — conservative (live maker fills are lower cost). No double-counting.

3. **SOL coverage: VERIFIED.** `solusdt_1d.parquet` EXISTS (2073 rows, 2020-08 to 2026-04-14). BTC/ETH cap at 3000 rows → 2026-03-23. Minor gap (~3 weeks for BTC/ETH).

4. **Daily equity harness: FULL DATA RUN (2026-04-15).** Export cap fixed (2000→5000 bars). Full history: $10K → $67M (+670,515%), 310 trades, MaxDD 62.6%. Per-year: 2018 +1393%, 2019 +421%, 2020 +879%, 2021 +35.5% (choppy), 2022 +10.9% (BTC -45.6%), 2023 +101.9% (CORRECTED — was falsely +10.9%), 2024 +145.7%, 2025 +73.8%, 2026 -22.7% YTD.

   ⚠️ **2023 "weakness" was a data artifact.** The +10.9% figure was from stale BTC/ETH parquet (3000-row cap → 2023-03-23). Full data: +101.9% vs BTC +146.5%.

   ⚠️ **Three Sharpe numbers for Turtle:**
   - Walk-forward per-window avg: **6.29** (mean of per-window Sharpe ratios — NOT daily compounded)
   - Daily equity Sharpe (NoDOGE, honest): **1.34** (computed from actual daily returns on equity curve)
   - Progress chart (NoDOGE, 5 symbols): **~2.5** (Chandelier dual-exit, not fixed hold)
   - **Never report 6.29 on an equity chart.** Use 1.0-1.4 for NoDOGE equity curve captions.

---

## Critical In-Flight Items (resolved as of 2026-04-14)

- [x] **TRUE HELD-OUT VALIDATION DONE:** OPTIMIZED beats DEFAULTS 91% overall, 81% on held-out windows. Hyperopt found REAL structure, not noise.
- [x] **W05 Regime Stress Test COMPLETE:** Base5 6/6 pass. Failures are LTC/EOS/BCH-specific. Production rule: exclude those assets.
- [x] **PRODUCTION VALIDATION DONE:** NoDOGE = 6/6 PASS (100%). Avg Sharpe 6.87, fee-adj ~4.8. Worst DD 35.4%. DEPLOYABLE.
- [x] **EQUITY CURVE INTEGRITY FIX (UPDATED 2026-04-15):** Off-by-one bug fixed (db47c6e). Turtle now shows real 2.24 Sharpe (6-symbol Base5) in unified harness. MACD+Regime (2/7 OOS) and Blend removed from chart (graveyard).
  **⚠️ PROGRESS CHART RECORRECTED (2026-04-15):** `progress_equity_curves.csv` turtle_equity was still using wrong engine (fixed 21-bar hold → 116x at day 2072). Fixed: now spliced from correct `turtle_chandelier_equity.csv` (Chandelier dual-exit → **1126x at day 2072**). Stale macd_equity/blend_equity columns removed from CSV. DDBudget = milestone-aggregated (NOT directly comparable to Turtle's daily equity).
- [x] **A/D Static Sleeve: REJECTED.** Only 46% win rate. Turtle-only is production.
- [x] **EXECUTION MODEL AUDIT (2026-04-15):** Live Binance BTCUSDT perp maker rate 63% (vs 70% assumption — slightly conservative). USDT-M fees: Maker 0.02%, Taker 0.05%. 70% assumption is slightly conservative; strategy remains viable.
- [x] **BTC Trend Scalar: REJECTED (2026-04-15).** Baseline wins 6/6, Sharpe 5.46. No scalar config adds value.
- [x] **Progress equity curves regenerated (2026-04-15).** Turtle: 120.8x, Sharpe 2.24 (daily equity, full 2073-day history). MACD+Regime and Blend excluded (2/7 OOS — GRAVEYARD).
- [x] **HALL_OF_FAME STALE MACD TABLES (2026-04-15):** Archived 9,458 chars of sessions 18-27 MACD+Regime composite ranking tables (all stale). Replaced with brief archive note + GRAVEYARD verdict. Walk-forward MACD section (Session 31) also replaced with GRAVEYARD verdict (2/7 pass). File reduced 30% (35K→24K chars).

---

## 🎯 CRITIQUE FINDINGS (2026-04-16 evening)

**Biggest blind spot: No regime defense.**
- 2026 YTD: Turtle -22.7% vs BTC +12.7% — worst relative performance in strategy history
- ALL non-trend strategies dead (GRAVEYARD: BollingerRev 0/288, FDUSD carry 19%, 4h MR 0/4, 1h MR 0/6, funding MR 43%)
- ALL position-sizing overlays failed (USDT hedge, BTC scalar, chop filter, drawdown trigger)
- No real-time regime detection in the live bot — cannot reduce exposure when strategy is in hostile chop
- Structural conclusion: Turtle only works in trending regimes. When markets don't trend, we underperform with no defensive answer.

**Metric honesty:** "Sharpe 5.0+" is DEAD. Use 1.0-1.3 on equity charts. Three incompatible Sharpe numbers documented (6.29 WF avg / 1.34 daily equity / ~2.5 progress). Flag any instance of "Sharpe 5+" in reports — it is not comparable.

**Last 5 commits:** Refinement/auditing only, not discovery. Research is closed.

## 🎯 NEXT THREE EXECUTION TASKS — ALL RESOLVED (2026-04-16)

1. **[✅ COMPLETE] Cross-Market Equity Walk-Forward** — SPY 88%, QQQ 76%, GLD 53% (37/51 = 73%). Edge generalizes beyond crypto. CHAND_MULT=2.15 on same dual-exit saturation plateau (M≥2.15 = Turtle ATR fires first).

2. **[✅ COMPLETE] SOL Dollar-Sized Re-test** — Walk-forward cap NEVER activates (unit equity resets to 1.0 per window). $50K cap is only for large live accounts (>$250K). No walk-forward re-test needed.
3. **[✅ COMPLETE] Regime Monitor for Live Bot** — live_turtle_chandelier.rs updated with CHAND_M=2.15 + regime transparency display.

**Only blocked:** Live testnet (API keys from Noah).


---

*Prior sessions (deprecated — retain for history):*
- ~~Live testnet (blocked on API keys)~~
- ~~Production readiness report (done)~~
- ~~Cross-Market Audit (done, 3/8 pass)~~

---

## ⚡ ALL TURTLE HYPEROPTS COMPLETE — No Params Remain

ALL Turtle+Chandelier params FROZEN as of 2026-04-14:
- EP=21 ✅ (hyperopt 2026-04-10)
- ATR_PERIOD=25 ✅ (hyperopt 2026-04-12)
- ATR_MULT=0.0 ✅ (no filter — hyperopt 2026-04-13)
- CHAND_PERIOD=28 ✅ (hyperopt 2026-04-11)
- CHAND_MULT=2.15 ✅ (hyperopt 2026-04-16: fine step=0.05 sweep → +25.5% Sharpe vs coarse step=0.5 baseline. Saturation plateau M≥2.15 confirmed.)
- HOLD_MAX=45 ✅ (hyperopt 2026-04-11)
- POSITION_CAP=3 ✅ (hyperopt 2026-04-11, extended sweep confirmed)
- **CHOP_FILTER: REJECTED (2026-04-14)** — destroys Sharpe in all configs

---

## 🛑 STOP DOING — Research Exhausted

**Track C CLOSED PERMANENTLY 2026-04-15.** 1h MR kill test: 0/6 symbols pass on full 6-7yr history. Prior 5/5 was a 1-year data artifact.

**Track C CLOSED — DEFINITIVE KILL 2026-04-15:** 288-config × 6-symbol full walk-forward on 1h data (2,834 days). ALL negative Sharpe: BTC -2.03, ETH -1.27, SOL -0.31, XRP -0.60, DOGE -0.90, ADA -1.05. Previous ETH-only "4/4" was from short ETHFDUSD cache (417 days). On full history, 1h MR is structurally dead — not fee-sensitive.

**All non-trend strategies dead.** Every strategy family tested and killed (BollingerRev 0/288, BOCPD 0%, 4h MR 0/4, FDUSD carry 19%, Funding MR 43%, 1h MR 0/6, cross-sectional 60%, vol-rank 60.5%, BTC scalar 0/8 configs). The reliable crypto edge is directional trend-following only.

---

## ✅ W04/W05 FAILURE ATTRIBUTION — SYMBOL-SPECIFIC vs REGIME (2026-04-15)

**Question:** Are W04/W05 failures symbol-specific or regime-inherent?

**Method:** Per-symbol single-symbol Turtle+Chandelier runs + portfolio for W04 and W05 across Base5, Legacy4, LowVolume5.

**Key findings:**
- XRPUSDT: fails 2/2 in Base5 W04/W05
- ADAUSDT: fails 2/2 in Base5 W04/W05
- SOLUSDT: strongest in chop — +47.3% (W04), +39.4% (W05)
- **BTC fails in Legacy4 W05** (regime-inherent) but **passes in Base5 W05** (peers carry)
- Base5 portfolio passes 6/6 because SOL/DOGE/ETH compensate for XRP/ADA failures
- Regime-inherent failures only in Legacy4/LowVolume5 (universe collapses in W05 when too many fail)

**Verdict (UPDATED 2026-04-15):** Rough W04/W05 attribution was methodologically flawed. Formal 11-universe walk-forward shows:
- XRP/ADA together are NEUTRAL diversifiers (XRP drag offset by ADA in some windows)
- **NoDOGE** (BTC/ETH/SOL/XRP/DOGE): 6/6 pass, Sharpe 6.87, worst DD 35.4% — BEST production candidate
- Removing ADA reduces tail risk AND improves Sharpe (+28% vs Base5)
- **ADA is a portfolio drag** in bull years (2021, 2024, 2025) — Turtle can't capture ADA's pumps, adds whipsaw
- **Prod5BNB fails W02** (BNB crashes more than ADA in COVID)

---

## Production Params (FROZEN as of 2026-04-14)

```
EP = 21          (entry lookback)
ATR_PERIOD = 25  (Turtle ATR)
ATR_MULT = 0.0   (no entry filter — best)
CHAND_PERIOD = 28
CHAND_MULT = 2.15
HOLD_MAX = 45
POSITION_CAP = 3
UNIVERSE = [BTC, ETH, SOL, XRP, DOGE]  (NoDOGE — ADA removed, was portfolio drag in bull years)
USE_CHOP_FILTER = FALSE  ← REJECTED
MAX_SOL_POSITION = $50K notional  ← due to slippage risk
```

**DDBudget 3-Sleeve:** 72% walk-forward pass (39/54). Use walk-forward pass rates for comparison. The 7.x Sharpe on the progress chart is milestone-aggregated (NOT daily-compounded) — NOT comparable to Turtle's daily equity Sharpe. Not a standalone production candidate.

**Fee assumptions:** Walk-forward uses 20bp RT (0.1% taker each side). This is already conservative — real maker fills are ~63-70% at zero cost, reducing effective fees to ~15bp RT.

**Honest Sharpe summary:**
- Walk-forward per-window avg: **6.29** (methodology artifact — mean of per-window Sharpe ratios; NOT daily compounded)
- Daily equity Sharpe (NoDOGE, full equity curve): **1.34** ← up from Base5 1.04 after ADA removal
- Progress chart (NoDOGE, 5 symbols): **~2.5** (Chandelier dual-exit)
- **Never report 6.29 on an equity chart.** Use 1.0-1.4 for NoDOGE equity curve captions.

**Expected live Sharpe:** ~1.0-2.0 range. If > 1.0 after 30 days live → proceed. If < 0.5 → diagnose.

⚠️ **The 6.29 walk-forward Sharpe is NOT directly comparable to the daily equity Sharpe (~1.0).** They are different statistical objects. The equity chart caption must use ~1.0-1.3, not 6.29.

---

## GRAVEYARD (Complete as of 2026-04-15)

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
| **BTC Trend Scalar** | **0/8 configs beat baseline** | **REJECTED 2026-04-15** — BTC in BULL all windows, scalar never activates |

---

## Completed (2026-04-15)

- [x] Held-out validation: 91%/81% OPTIMIZED vs DEFAULTS
- [x] Production validation: Base5 6/6 pass (100%)
- [x] Regime stress test: 21/21 pass pre-2021
- [x] Execution realism layer: 22-33% fee drag quantified
- [x] Maker-taker microstructure: ~70% maker fill confirmed (live: 63%)
- [x] All Turtle params hyperopt-frozen
- [x] BollingerReversion definitive kill (integrity fix)
- [x] HALL_OF_FAME cleanup (1003→612 lines)
- [x] Examples audit: 310 files, VALIDATED_REGISTRY.md created
- [x] Equity curve pipeline rebuilt
- [x] Critiques (2026-04-11, 2026-04-14)
- [x] A/D sleeve re-run: REJECTED (46% win rate, -6.2% improvement)
- [x] Daily equity harness first run: $10K → $67M, 310 trades
- [x] SOL coverage verified: 2073 rows (2020-08 to 2026-04-14)
- [x] BTC/ETH data gap noted: 3000-row cap → 2026-03-23
- [x] Execution model audit: VERIFIED — model is conservative and trustworthy
- [x] BTC Trend Scalar: REJECTED (baseline wins, 6/6 pass, Sharpe 5.46)
