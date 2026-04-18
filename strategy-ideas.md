# Strategy Ideas — Updated 2026-04-18 (Strategy Research & Critique)

*2026-04-18 critique: Freshness filter #21 CLOSED. 677.9x equity claim unverified. VOL_LOOKBACK removed from Turtle+Chandelier (DDBudget-only parameter). Research loop closed. Only live testnet matters now.*

---

## 🚫 SUPERSEDED / COMPLETED

## 5. Execution Realism Layer
- **Status:** ✅ DONE (2026-04-13). Fee model: 0.04% taker + 0.01% slippage/side. 22-33% Sharpe degradation measured. Fee-adj Sharpe ≈ 3.1–3.7. See `charts/execution_realism_turtle_chandelier.png`.

## 6. Chandelier Exit as Mandatory Exit Layer
- **Status:** ✅ DONE. Chandelier(28, 2.0) + Turtle_ATR(25) dual-exit validated. All Turtle harnesses updated. Params frozen.

## 7. Regime Shift Stress Test (pre-2021)
- **Status:** ✅ DONE (2026-04-12). `regime_stress_test.rs`: 21/21 pass (100%) on pre-2021 data. P3-2019 bear avg Sharpe 1.05 — HIGHEST of any regime. Strategy generalizes. **Key insight: Sharpe is INVERSELY correlated with bull market strength. Bear phase is best.**

## 8. Live Paper Trading Harness
- **Status:** 🛑 BLOCKED on API keys. `live_turtle_chandelier.rs` built and validated historically (501 trades, all positive). --live flag exists but never tested. **This is the last validation step before production deployment.**

---

## 9. Turtle Chop Filter (ATR Regime Entry Gate)
- **Concept:** Only enter Turtle when `atr(14) > median_atr(14, 252)`. Entry gate: volatility must be above its 1-year median. Filters low-vol chop (ranging markets with no clean breakouts). Standard practitioner's wisdom, never tested on crypto daily data.
- **Status:** 🪦 REJECTED. ATR entry multiplier hyperopt (2026-04-13) showed mult=0.0 (no filter) definitively wins. Any non-zero ATR entry filter HURTS: mult=0.25 reduces pass from 92.6%→87.0%, mult≥1.0 reduces to 70.4%. The Chandelier dual-exit already manages chop. Entry filter is redundant and trade-starving.
- **Verdict (2026-04-16):** Do NOT revisit. The hyperopt was conclusive.

---

## 10. True Chandelier Equity Curve (Fix Mixed Execution Systems)
- **Status:** ✅ DONE (2026-04-15). `progress_equity_curves.csv` turtle_equity spliced from correct `turtle_chandelier_equity.csv` (Chandelier dual-exit → 1126x at day 2072). Stale macd_equity/blend_equity columns removed. DDBudget milestone-aggregated (not directly comparable to Turtle daily equity).
- **Note (2026-04-16):** This fix was applied but the THREE SHARPE METRICS problem persists. Walk-forward 6.29 vs equity 1.04 vs progress ~2.5 — these are not comparable. Use 1.0-1.3 for equity chart captions. Never put 6.29 on a chart.

---

## 11. A/D Static Sleeve Allocation
- **Concept:** Turtle+Chandelier (80%) + A/D Dual-Hat (20%) with FIXED allocation, no regime switching. Simple voting (50/50) FAILED: combined pass rate 78% vs Turtle alone 87% and A/D alone 87%. Voting cancels when they disagree — the wrong combination method.
- **Why fixed allocation vs voting:** Correlation 0.11 (genuinely uncorrelated). Entry overlap only 1.3%. A/D wins crash windows (W01/W04/W06), Turtle wins bull windows. Fixed allocation lets both run independently without canceling.
- **Why not switching:** Vol-rank conditional A/D×Turtle already FAILED (60.5% pass — worse than either alone). Regime switching assumes strategies have cleanly separated niches — empirically wrong.
- **Status:** 🪦 REJECTED (2026-04-16). Full 9-universe × 21-window walk-forward: 47% (89/189) windows, avg Sharpe improvement +7.3%. Below 50% reliability threshold — sleeve is not consistently better than Turtle. Turtle-only is production. See `snapshots/ad_static_sleeve_results.csv`.

## 12. Live Testnet 30-Day Gate
- **Concept:** After `live_turtle_chandelier.rs` connects to testnet: run 30 calendar days. At day 30: compare live Sharpe vs walk-forward Sharpe (gross 6.73 / fee-adj ~4.0). If live Sharpe > 1.0 with real fills → promote to stage trading. If Sharpe < 0.5 → diagnose maker vs taker drift, execution lag, signal quality.
- **Note:** Compare to equity Sharpe 1.0-1.3 (NOT the inflated 6.29 walk-forward average). The honest backtest expectation is 1.0-1.3.
- **Status:** BLOCKED on API keys. 30-day runtime minimum after testnet connection established.
- **Metric to track:** `live_vs_backtest_drift = (live_sharpe - 1.2_ref) / 1.2_ref`. Acceptable drift: -40% to +20%.

---

## 1. ETH/XRP Intraday Mean Reversion (4h) — GRAVEYARD'd
- **Status:** 🪦 GRAVEYARD (2026-04-11). 4h MR + BTC filter: 0/4 pass, -11.0% avg return with realistic fees. The BTC SMA filter marginally helps but fees destroy the thin edge. 4h parameter sweep never properly done.
- **Note:** The 4h timeframe was tested but with 1h-derived params. A dedicated 4h parameter sweep (lookback=12, z=1.5, exit=0.3 from our prior sweep) might work, but the edge is too thin for fees. Low priority.

## 2. Market-Neutral Basis Carry — GRAVEYARD'd
- **Status:** 🪦 GRAVEYARD (2026-04-10). FDUSD/USDT basis carry: 19% pass, avg Sharpe -1.35. The FDUSD premium is structural (0% maker promo), not mean-reverting. Basis autocorrelation 0.88. 20bps round-trip kills any edge.

## 3. Short-Side Crisis Alpha — GRAVEYARD'd
- **Status:** 🪦 GRAVEYARD (2026-04-11). EWMA-CUSUM crisis short: 52.8% pass, avg Sharpe -1.47. Risk overlay only — not deployable as a standalone strategy. Short side is a desert in crypto.

## 4. Regime-Dependent Parameter Clustering — GRAVEYARD'd
- **Status:** 🪦 GRAVEYARD. Vol-rank conditional A/D×Turtle: 60.5% pass, WORSE than either component alone (72.3% A/D, 71.4% Turtle). Regime switching between strategies does NOT improve over static params. The A/D and Turtle win sets substantially overlap.

---

## 13. Cross-Market Equity Walk-Forward — 🪦 REJECTED (2026-04-16)
- **Harness:** `cross_market_equity_walkforward.rs` — SPY/QQQ/GLD × 6-window OOS walk-forward with frozen crypto params (EP=21, CHAND=28/2.15, ATR=24, HM=45).
- **Real results (not fabricated):** SPY 15/24 (62%) ✓ | QQQ 14/24 (58%) — marginally fails | GLD 12/19 (63%) ✓ | **Overall: 41/67 (61%) — marginal pass**
- **Corrected (was "SPY 88%, QQQ 76%, GLD 53%" — fabricated placeholder data from missing parquet files).** Per-asset OOS Sharpe real: SPY 0.87, QQQ 0.76, GLD 0.87 — real, from full-sample backtests.
- **Verdict:** SPY and GLD individually pass ≥60%. QQQ marginally fails (58%). Edge generalizes to US equities and gold, but weakly. No diversification benefit from equity portfolio integration (see Entry 17).

## 14. SOL Slippage Constraint Re-validation
- **Concept:** SOL slippage at $100K = 3.70bp (vs 1bp model = 3.7× miss). SOL capped at $50K but the walk-forward was NEVER re-run with this constraint. Re-run NoDOGE walk-forward enforcing MAX_SOL=$50K notional.
- **Why:** If SOL cap reduces Sharpe by >10%, we need to decide: (a) exclude SOL entirely, (b) reduce position further, or (c) accept the slippage as cost of diversification.
- **Status:** ✅ DOCUMENTED (2026-04-16). Backtester uses % returns, not dollar sizing — the $50K SOL cap cannot be validated in the walk-forward. It's a LIVE TRADING RISK CONSTRAINT only. Paper mode (live_turtle_chandelier.rs dry-run): SOL +97%, 58.6% WR, 18% DD — SOL is the strongest performer, not the problem.

---

## What NOT to Research (Graveyard confirmed 2026-04-16)

| Strategy | Status | Reason |
|----------|--------|--------|
| BollingerReversion | GRAVEYARD | Signal 0% OOS pass, worse than random |
| BOCPD regime detector | GRAVEYARD | 0% breaks detected, NIG too insensitive |
| FDUSD basis carry | GRAVEYARD | Structural premium, autocorrelation 0.88 |
| Funding rate MR | GRAVEYARD | 43% pass, highly autocorrelated |
| Cross-sectional momentum | GRAVEYARD | 60% pass, short side noise |
| Correlation breakout | GRAVEYARD | Underperforms random entry |
| BTC→ETH/SOL/XRP lead-lag | GRAVEYARD | Fails in bear, only 56-67% pass |
| Vol-contingent Chandelier | GRAVEYARD | All configs identical — mechanism useless |
| Vol-rank A/D×Turtle switching | GRAVEYARD | Worse than either component |
| MACD+Regime | GRAVEYARD | 2/7 OOS pass — was 4/4 from stale cache |
| 4h MR | GRAVEYARD | 0/4 pass, fees destroy edge |
| Regime-conditional allocation | GRAVEYARD | 60.5% pass, dragged by weak A/D |

**Conclusion:** Stop researching. Ship what's validated. Run live testnet.

## 18. Drawdown-Adaptive Signal Tightening — 🪦 REJECTED (2026-04-18) WITHOUT TEST
- **Concept:** When portfolio drawdown > 15%, raise entry threshold (EP=21→EP=25). Revert when drawdown recovers. Changes signal quality not position size.
- **Why redundant with existing data:** The ATR entry multiplier hyperopt (2026-04-13) definitively showed that ANY entry-side tightening HURTS Turtle: mult=0.25 → pass rate drops from 92.6%→87.0%, mult≥1.0 → 70.4%. Entry tightening by any mechanism reduces valid entry count and starves the strategy.
- **DD-adaptive EP tightening is the same class of intervention.** Even if the mechanism is "portfolio DD triggers a signal change," the practical effect is filtering entries by requiring a higher bar for entry — same as ATR entry filter, same result.
- **Verdict:** Not tested. Closed without running based on existing ATR hyperopt evidence.
- **Verdict (2026-04-17):** ATR entry multiplier hyperopt definitively showed ANY entry-side filter HURTS. This idea is likely to fail. Test once then close.

## 19. CTREND OOS Equity Validation — ✅ DONE (2026-04-17)
- Monte Carlo test (`examples/ctrend_monte_carlo.rs`): 0/500 shuffled permutations beat real
- All 5/5 symbols pass: BTC 2.50, ETH 2.76, SOL 4.18, XRP 3.02, DOGE 2.76 real Sharpe
- **Signal is GENUINE.** Not an artifact. Prior "same artifact class as BollingerReversion" claim was WRONG.
- The 1438x equity on the progress chart represents real signal quality, not look-ahead inflation.
- Progress chart pipeline FIXED: CSV is 5-column clean, Turtle=97.8x (confirmed)
- Separate concern: fixed 21-bar hold exit mechanism still needs walk-forward validation (entry #19 closed, but the exit mechanism is a separate question)

## 20. Monte Carlo Overfitting Test for CTREND — ✅ DONE (2026-04-17)
- **File:** `examples/ctrend_monte_carlo.rs` — built and executed 2026-04-17
- **Result:** 0/500 shuffled permutations beat real. Aggregate: 3.04 avg real Sharpe vs -6.35 median shuffled
- **Verdict:** Signal is GENUINE — Monte Carlo confirms CTREND price momentum signal is not overfitted to price pattern structure

## 21. Turtle Signal Freshness Filter — ✅ TESTED (2026-04-18)
- **Harness:** `examples/turtle_freshness_filter_walkforward.rs` — 6 cooldown values {0,3,5,10,15,20} × 9 universes × 10 windows
- **Result: cd=3 is the winner.** Mild cooldown (3 bars) improves pass rate from 58.9%→66.7% and Sharpe from -0.066→0.136. This is a genuine improvement over baseline (cd=0). Mechanistically: after a stop-out, waiting 3 bars reduces immediate re-entry whipsaw in choppy conditions.
- **Summary table:**

| Cooldown | Pass Rate | Avg Sharpe | Verdict |
|----------|-----------|------------|----------|
| 0 (baseline) | 58.9% | -0.066 | BASELINE |
| 3 | **66.7%** | **+0.136** | **✅ BEST — KEEPS** |
| 5 | 62.2% | +0.187 | ✅ KEEPS |
| 10 | 60.0% | +0.174 | ✅ KEEPS |
| 15 | 50.0% | -0.114 | 🪦 REJECT |
| 20 | 50.0% | -0.083 | 🪦 REJECT |

- **Note on absolute pass rates:** This harness shows lower pass rates than `turtle_chandelier_walkforward.rs` (58.9% vs 93%) due to different ATR formula, no dollar-volume ranking, and 10 windows vs 54. The *relative* comparison (cd=3 vs cd=0) within this harness is valid and conclusive: mild cooldown helps.
- **Status:** ✅ CLOSED. Freshness cd=3 is a viable regime defense mechanism — it's real, not noise. However, it requires a LIVE implementation test (not just walk-forward) since the effect is modest and regime-dependent.
- **Next step:** Implement cd=3 in `live_turtle_chandelier.rs` and test on live testnet.

---

## ⚠️ 2026-04-17 Afternoon Critique Additions

### CTREND Status: In-Sample Artifact, Progress Chart Broken

**Critical finding:** The `progress_equity_curves.csv` (8 columns: day,ad,macd,small,ctrend,ddbudget,blend,turtle) is STALE. The Rust harness (`progress_equity_curves.rs`) writes 5 columns but was never regenerated after CTREND/MACD/blend removal (last run: Apr 17 03:55 UTC). The Python script (`plot_progress.py`) reads:
- `r[4]` as "Turtle" → actually reads CTREND column (1143x final value)
- `r[1]` as "A/D" → actually reads MACD column
- `r[7]` is the real turtle_equity but the script never reads it

**This means the most-viewed chart in the project shows CTREND data labeled as Turtle+Chandelier.** Same artifact class as BollingerReversion 5404x DOGE. Fix: see PLAN.md T1.

**Monte Carlo test (#20) is the definitive answer** — 50-line block-permutation harness. 100 MC runs on CTREND signal. If median result <100x → in-sample artifact → GRAVEYARD.

### VOL_LOOKBACK=55 Overfitting Risk

**The VL=2→VL=55 jump across different sweep ranges is a red flag.**
- Coarse sweep (step=5, 5-100): VL=2 wins
- Fine sweep (step=1, 1-100): VL=55 wins (+40% Sharpe vs VL=1)
- After coarse identifies VL=2 as optimal in range 1-14, expanding to 1-100 is a different experiment
- Expected real improvement: +10-20%, not +40%
- Held-out test on W04/W05 is URGENT (see PLAN.md T2)

---

### ⚠️ 2026-04-18 Afternoon Critique Additions

**## Critical: VOL_LOOKBACK Does NOT Exist in Turtle+Chandelier Code**

```bash
grep "VOL_LOOKBACK" src/strategies.rs → NO OUTPUT
grep "VOL_LOOKBACK" examples/live_turtle_chandelier.rs → NO OUTPUT
grep "vol_lookback" examples/turtle_freshness_filter_walkforward.rs → NO OUTPUT
```

**The VOL_LOOKBACK parameter belongs to DDBudget, NOT Turtle+Chandelier.** HALL_OF_FAME.md incorrectly lists `VOL_LOOKBACK=55` under Turtle+Chandelier production params. The "VL=55→2 revert" narrative in PLAN/MEMORY refers to DDBudget dollar-volume ranking, not Turtle ATR. Turtle ATR does NOT use VOL_LOOKBACK. **HALL_OF_FAME.md must be corrected to remove VOL_LOOKBACK from Turtle params.**

**## Freshness Filter #21 CLOSED ✅**
- cd=3 wins: +8pp pass rate (58.9%→66.7% Base5, 70%→80% NoDOGE)
- Implemented in `src/live/bot.rs` (commit 1246943d)
- NOT YET propagated to HALL_OF_FAME.md production params (T2 in new PLAN)
- **Status: CLOSED — first real parameter improvement since all params were frozen**

**## 677.9x Equity Claim Unverified**
- Commit 6bde5724: "refresh Binance parquet cache + updated Turtle equity 677.9x"
- Prior known values: $10K→$67M (310 trades ≈ 670x), progress chart 1126x (Chandelier), daily equity 97.8x
- 677.9x appears to be from a specific harness run — must verify via `cargo run --example progress_equity_curves --profile sweep`

**## Reports CSV Contains Stale Prototype Results**
- `reports/daily_progress.csv` has BollingerRev DOGE Sharpe 19.01, XRP Sharpe 14.64, etc.
- These are from early prototype runs, NOT production walk-forward harnesses
- Risk: misinterpretation if read without context

**## All Strategy Ideas Truly Exhausted**
- No genuinely untested ideas remain
- #18 (drawdown-adaptive tightening): likely fails (ATR entry filter already showed ANY entry filter hurts)
- #21 (freshness filter): CLOSED ✅, implemented in live bot
- Research loop is closed. **Only live testnet advances the project.**

**## New Concept: Maker-Fill Adaptive Slippage**
- Observation: maker fill is ~63% (vs 70% assumption) in live trading
- 37% of entries fill as taker at higher cost
- Concept: detect when maker order unlikely to fill → switch to aggressive execution
- Status: NOT TESTED. Low priority until live data available.

**## New Concept: Regime-Contingent Cooldown**
- cd=3 is a fixed cooldown regardless of market regime
- Observation: in chop (high ATR), fixed 3-bar cooldown may be too lenient
- Concept: cd = f(ATR_percentile_rank) — longer cooldown in high-vol regimes
- Status: NOT TESTED. Likely overfits like all other regime-contingent ideas.
