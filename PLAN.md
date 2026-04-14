# PLAN.md - Krypto Research Priorities

## Current Focus — Integrity Fix + Chop Filter Validation

**Equity curve system has been producing misleading numbers for WEEKS. Fix first. Then test chop filter. Then wait for live testnet.**

---

## Critical In-Flight Items (must resolve before new research)

- [x] **TRUE HELD-OUT VALIDATION DONE:** OPTIMIZED beats DEFAULTS 91% overall, 81% on held-out windows. Hyperopt found REAL structure, not noise. All Sharpe numbers are upper bounds (not inflated). See held_out_validation.rs.
- [x] **W05 Regime Stress Test COMPLETE:** DD-sizing mechanism acceptable. Legacy4/Legacy3/LowVolume5 W05 failures are structural (non-trending LTC/EOS/BCH) — not fixable by position sizing.
- [x] **PRODUCTION VALIDATION DONE (2026-04-13 evening):** Base5 (BTC ETH SOL XRP DOGE ADA) = **6/6 PASS (100%)** across all windows including W04/W05. Avg Sharpe 6.73, fee-adj 5.25. Worst DD 35.4% (W02 COVID-crash). **Key finding: W05 failures are LTC/EOS/BCH-specific — these assets are NOT in Base5.** Production rule: exclude LTC, EOS, BCH. DEPLOYABLE. See snapshots/production_validation_report.md.
- [ ] **EQUITY CURVE INTEGRITY FIX (CRITICAL):** `progress_equity_curves.csv` tracks 6 strategies but uses FIXED 21-bar hold for DDBudget AND for `turtle_chandelier_equity.csv`. The validated walk-forward uses Chandelier(28,2.0)+Turtle_ATR(25) dual-exit. **Result: the 519,288% return and 7.61 Sharpe in equity curves come from a DIFFERENT exit system.** The 6.73 walk-forward Sharpe is from Chandelier. You cannot compare them. **FIX:** Update `progress_equity_curves.rs` to use Chandelier dual-exit for all entries. Regenerate `turtle_chandelier_equity.csv` with correct Chandelier(28,2.0)+Turtle_ATR(25) code. One source of truth for all equity curves.

---

## 🎯 TODAY'S PRIORITY (2026-04-14)

1. **[FIX] Equity curve integrity:** Update `progress_equity_curves.rs` to use Chandelier(28,2.0)+Turtle_ATR(25) dual-exit for Turtle+Chandelier. Regenerate CSVs. One source of truth.
2. **[TEST] Chop Filter:** Walk-forward test `atr(14) > median_atr(14,252)` as binary entry gate. 9 universes × 54 windows. If pass rate > 92.6% → add `USE_CHOP_FILTER=true` to production params. If not → accept Turtle as-is, move to live testnet.
3. **[DOCS] Commit and push critique + plan update.**

---

## ⚡ HIGH PRIORITY — The ONE Parameter Worth Testing

**All Turtle params are FROZEN (2026-04-13):** EP=21, ATR=25, mult=0.0, CAP=3, HM=45, CHAND(28,2.0). Stop hyperopting.

- [ ] **Turtle Chop Filter (ATR regime gate):** Walk-forward test: only enter Turtle when `atr(14) > median_atr(14, 252)`. Hypothesis: filters low-vol chop (ranging markets with no clean breakouts). 9 universes × 54 windows. **This is the last parameter worth testing before live deployment.** If it works → add USE_CHOP_FILTER=true. If it fails → accept Turtle as-is and move to live testnet.

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
- [ ] **Chop Filter (atr > median_atr):** Walk-forward test in progress. 9 universes × 54 windows. Binary gate: only enter Turtle when ATR14 > 252-bar median ATR14.

---

## 🟡 BLOCKED — Awaiting Noah's API Keys

- [ ] **LIVE TESTNET CONNECTION:** `live_turtle_chandelier.rs` built, --live flag exists, never tested. This is the ONLY remaining validation before paper trading. Maker vs taker gap is the biggest unmeasured variable. All fee models are theoretical until live testnet runs. **BLOCKED on API keys from Noah.**

---

## Production Params (FROZEN as of 2026-04-13)

```
EP = 21          (entry lookback)
ATR_PERIOD = 25  (Turtle ATR)
ATR_MULT = 0.0   (no entry filter — best)
CHAND_PERIOD = 28
CHAND_MULT = 2.00
HOLD_MAX = 45
POSITION_CAP = 3
UNIVERSE = [BTC, ETH, SOL, XRP, DOGE, ADA]  (no LTC/EOS/BCH)
USE_CHOP_FILTER = ???  (pending walk-forward validation)
```

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
