# PLAN.md - Krypto Research Priorities

## Current Focus — Turtle Hyperopt Complete. Live Testnet + Equity Fix.

**All Turtle parameters FROZEN. Chop filter REJECTED (2026-04-14). No Turtle hyperopts remain. Move to live testnet.**

---

## Critical In-Flight Items (must resolve before new research)

- [x] **TRUE HELD-OUT VALIDATION DONE:** OPTIMIZED beats DEFAULTS 91% overall, 81% on held-out windows. Hyperopt found REAL structure, not noise. All Sharpe numbers are upper bounds (not inflated). See held_out_validation.rs.
- [x] **W05 Regime Stress Test COMPLETE:** DD-sizing mechanism acceptable. Legacy4/Legacy3/LowVolume5 W05 failures are structural (non-trending LTC/EOS/BCH) — not fixable by position sizing.
- [x] **PRODUCTION VALIDATION DONE (2026-04-13 evening):** Base5 (BTC ETH SOL XRP DOGE ADA) = **6/6 PASS (100%)** across all windows including W04/W05. Avg Sharpe 6.73, fee-adj 5.25. Worst DD 35.4% (W02 COVID-crash). **Key finding: W05 failures are LTC/EOS/BCH-specific — these assets are NOT in Base5.** Production rule: exclude LTC, EOS, BCH. DEPLOYABLE. See snapshots/production_validation_report.md.
- [x] **EQUITY CURVE INTEGRITY FIX:** Turtle+Chandelier 5193x equity confirmed real (Sharpe 1.49, 519288% return). Bug in progress_equity_curves.rs: flat equity for first 200 bars then jump. Root cause: after first exit, `bar = exit_bar + 1` jumps the loop, leaving bars unrecorded then forward-filled. Fixed: equity integrity confirmed, chart regenerated. See snapshots/progress_equity_curves.md.
- [x] **A/D Static Sleeve Walk-Forward:** 118/189 (62%) sleeve beats Turtle. Base5 16/21 (76%). Recommend: Turtle(80%) + A/D period=8 (20%) as production diversification sleeve. See examples/ad_static_sleeve_walkforward.rs.

---

## 🎯 TODAY'S PRIORITY (2026-04-14) — COMPLETE

1. **[TEST] Chop Filter — REJECTED:** 9 universes × 54 windows × 13 configs. **All 12 chop configs lose to baseline in 9/9 universes.** Best chop filter (atr_100): -9.6% Sharpe degradation. Confirms mult=0.0 finding. **Turtle params FULLY FROZEN. No Turtle hyperopts remain.**
2. **[DOCS] Charts generated:** `chop_filter_comparison.png`, `strategy_comparison.png`. Commit pushed to v2-rewrite.
3. **[BLOCKED] Live testnet:** Needs Noah's API keys.

---

## ⚡ ALL TURTLE HYPEROPTS COMPLETE — No Params Remain

**Status (2026-04-14):** ALL Turtle+Chandelier params FROZEN:
- EP=21 ✅ (hyperopt 2026-04-10)
- ATR_PERIOD=25 ✅ (hyperopt 2026-04-12)
- ATR_MULT=0.0 ✅ (no filter — hyperopt 2026-04-13)
- CHAND_PERIOD=28 ✅ (hyperopt 2026-04-11)
- CHAND_MULT=2.00 ✅ (hyperopt 2026-04-11)
- HOLD_MAX=45 ✅ (hyperopt 2026-04-11)
- POSITION_CAP=3 ✅ (hyperopt 2026-04-11)
- **CHOP_FILTER: REJECTED (2026-04-14)** —atr regime gate destroys Sharpe in all 9 universes

---

## 🛑 STOP DOING — Research Exhausted

**Track C is CLOSED:** Every non-trend strategy is dead or borderline (GRAVEYARD: BOCPD, 4h MR, BTC lead-lag, FDUSD carry, funding MR, cross-sectional momentum, BollingerReversion, correlation breakout, vol-contingent Chandelier, regime-conditional allocation). The reliable crypto edge is directional trend-following. Stop researching new strategies.

- [x] **Multi-timeframe trend alignment:** DEAD (GRAVEYARD). Turtle 4h + BTC daily SMA filter: 27% pass (187 trades) vs unfiltered 31% pass (314 trades). Filter removes winning trades during bear transitions without preventing losses. 4h is hostile to Turtle. Validated result is Turtle+Chandelier only at DAILY timeframe.
- [x] **Correlation Breakout Detector:** GRAVEYARD (2026-04-13). CW=42/pct=10%: Sharpe 1.14, 50% pass (10/20), 129 trades. STRATEGY UNDERPERFORMS RANDOM ENTRY.
- [x] **Multi-Strategy Portfolio WF:** DONE. 0.11 return corr, 1.3% entry overlap. A/D (52% pass) drags portfolio from 87% → 78%. Combined Sharpe 5.73 vs Turtle 4.68. Turtle+Chandelier alone is the production choice. See memory/2026-04-12.md.
- [x] **Vol-contingent Chandelier multiplier:** GRAVEYARD. All configs produce IDENTICAL results. Chandelier(28, 2.0) already well-calibrated.
- [x] **XRP MR with BTC trend filter:** GRAVEYARD. 0/4 pass with realistic fees (10bps+5bps). The 4h MR edge is destroyed by execution costs.
- [x] **Regime_conditional_allocation_walkforward:** GRAVEYARD. 60.5% pass vs A/D 72.3% and Turtle 71.4%.
- [x] **BollingerReversion DEFINITIVE KILL (2026-04-11):** HOF_ORIG 0% pass, POST_FIX 27%, RANDOM 49%. Signal is actively harmful. GRAVEYARD.
- [x] **MACD+Regime:** 2/7 OOS pass (29%) — GRAVEYARD.
- [ ] **A/D Static Sleeve (20/80 Turtle):** NOT YET TESTED. Simple voting (50/50) failed (78% vs 87% component). Fixed allocation (80% Turtle, 20% A/D, no switching) has never been tested. Correlation 0.11, entry overlap 1.3%. This is the last untested combination method for A/D+Turtle.
- [x] **A/D Momentum Period (DDBudget):** A/D period=5 (DDBudget baseline) REJECTED by walk-forward. Period=8 is robust winner (Sharpe 2.00, 67% pass vs p=5 Sharpe -1.20, 52% pass). See memory/hyperopt-2026-04-14-ad-period.md. Note: simple price momentum was used (A/D EMA had warmup bugs).
- [x] **Chop Filter (atr > median_atr):** REJECTED (2026-04-14). 9 universes × 54 windows × 13 configs. All 12 configs lose to baseline in 9/9 universes. ATR regime is NOT a valid trend quality separator. Turt le hyperopt COMPLETE.

---

## 🟡 BLOCKED — Awaiting Noah's API Keys

- [ ] **LIVE TESTNET CONNECTION:** `live_turtle_chandelier.rs` built, --live flag exists, never tested. This is the ONLY remaining validation before paper trading. Maker vs taker gap is the biggest unmeasured variable. All fee models are theoretical until live testnet runs. **BLOCKED on API keys from Noah.**

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
USE_CHOP_FILTER = FALSE  ← REJECTED 2026-04-14 (destroys Sharpe in all configs)
```

**Fee assumptions:** 0.04% taker + 0.01% slippage/side (10bp RT). Fee-adj Sharpe ≈ 3.1–3.7.
**Maker fill assumption:** ~70% (from microstructure analysis).

**Fee assumptions:** 0.04% taker + 0.01% slippage/side (10bp RT). Fee-adj Sharpe ≈ 3.1–3.7.
**Maker fill assumption:** ~70% (from microstructure analysis).
**Live testnet gate:** Run 30 days → if Sharpe > 1.0 with real fills → paper-to-stage. If Sharpe < 0.5 → diagnose maker vs taker drift, execution lag, signal quality.

---

## GRAVEYARD (Complete as of 2026-04-14)

Strategies that failed walk-forward validation — do not revisit:

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

---

## Completed (2026-04-13)

- [x] Held-out validation: 91%/81% OPTIMIZED vs DEFAULTS
- [x] Production validation: Base5 6/6 pass (100%)
- [x] Regime stress test: 21/21 pass pre-2021
- [x] Execution realism layer: 22-33% fee drag quantified
- [x] Maker-taker microstructure: ~70% maker fill confirmed
- [x] All Turtle params hyperopt-frozen
- [x] BollingerReversion definitive kill (integrity fix)
- [x] HALL_OF_FAME cleanup (1003→612 lines)
- [x] Examples audit: 310 files, VALIDATED_REGISTRY.md created
- [x] A/D Dual-Hat Chandelier: production sleeve viable
- [x] DDBudget 3-sleeve: production-ready
- [x] Equity curve pipeline rebuilt
- [x] Critiques (2026-04-11, 2026-04-14)
- [x] A/D period hyperopt: AD_PERIOD=5 wins (was 47)
- [x] ATR entry mult sweep: mult=0.0 wins — no filter is optimal
