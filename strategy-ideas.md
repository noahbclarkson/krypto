# Strategy Ideas — Updated 2026-04-16 (Evening Critique)

*2026-04-16 evening critique: **CRITICAL** — CTREND 1438x on progress chart is likely in-sample artifact. ema_fast=50 default has negative OOS Sharpe (-13.017). ema_fast=60 winner still negative (-25.163) but 85.7% pass. The progress chart runs strategies on all bars without proper OOS windows. **Do NOT post CTREND equity to Discord until OOS validation completes.** See memory/2026-04-16-evening-critique.md for full critique.*

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

## 18. Drawdown-Adaptive Signal Tightening — NOT TESTED
- **Concept:** When portfolio drawdown > 15%, raise entry threshold (EP=21→EP=25) + add 4h SMA confirmation. Revert when drawdown recovers. Changes SIGNAL QUALITY not position size.
- **Why different from failed overlays:** USDT hedge/BTC scalar/drawdown trigger all changed risk budget (position size). This changes entry quality — tighten requirements when already underwater.
- **Risk:** ATR entry filter (conceptually similar) destroyed pass rate. Cautious — one test then graveyard if it fails.
- **Status:** NOT TESTED. Needs dedicated walk-forward harness.
- **Verdict (2026-04-17):** ATR entry multiplier hyperopt definitively showed ANY entry-side filter HURTS. This idea is likely to fail. Test once then close.

## 19. CTREND OOS Equity Validation — ⚠️ STILL INCOMPLETE (2026-04-17 04:18 UTC)
- **Concept:** `progress_equity_curves.rs` shows CTREND 1438x, Sharpe 5.17. This is an IN-SAMPLE artifact. The harness runs `StrategyKind::CTRend` with fixed 21-bar hold on ALL bars — no OOS windows.
- **What was done:** `dynamic_trend_walkforward.rs` with ema_fast=60 tested on Base5. Result: 6/7 pass (85.7%), Sharpe +2.01. The EMA crossover signal is GENUINE.
- **Critical problem (persisting since 2026-04-16 18:30 UTC):** PLAN.md claimed "CTREND → REMOVED from progress chart" but the PNG was NEVER regenerated. The chart STILL shows 1438x at 03:54 UTC today (04-17). **The fix was documented but not executed for 10+ hours.**
- **Remaining action:** Remove CTREND from `progress_equity_curves.rs` + `plot_progress.py` + regenerate CSV/PNG. Do NOT let another cycle pass with an in-sample artifact (1438x) displayed as if validated.
- **Status:** INCOMPLETE — chart fix not executed (2nd consecutive critique noting this).

## 20. Monte Carlo Overfitting Test for CTREND — URGENT (2026-04-16) — NOT BUILT
- **Concept:** CTREND's 1438x equity might be overfitted to specific price patterns in the data. Run Monte Carlo permutation test:
  1. Shuffle daily returns within each year-block (preserve volatility structure per year)
  2. Re-run CTREND signal on shuffled data 100 times
  3. If median shuffled result < 100x → artifact. If 1000x+ survives → genuine edge.
- **Why this vs walk-forward:** Walk-forward tests temporal stability. Monte Carlo tests overfitting to price pattern structure. This is the honest test — it's what caught BollingerReversion (+5404 DOGE was look-ahead contamination).
- **Method:** Generate synthetic price series via block-wise random permutation of returns. Run CTREND signal on synthetic series. Compare distribution of outcomes to real outcome (1438x).
- **Status:** NOT TESTED. No API keys needed. 6+ hours since marked URGENT.
- **File:** `examples/ctrend_monte_carlo.rs` (new)

## 21. Turtle Signal Freshness Filter — NOT TESTED (2026-04-17)
- **Concept:** Only enter a new Turtle position when the last exit (stop or HOLD_MAX) was at least X bars ago (e.g., X=5). Reduces re-entry whipsaw in chop — after a stop-out, require a cooldown period before re-entering the same symbol.
- **Why different from ATR entry filter:** ATR filter requires volatility above median — it FILTERS entries based on market conditions. Freshness filter requires TIME after exit — it FILTERS re-entries based on trade history. Mechanistically different.
- **Risk:** ATR entry filter hyperopt definitively showed ANY entry-side tightening HURTS (mult=0.25 → 87% pass from 92.6%). Freshness filter is entry filtering by a different mechanism but likely to also hurt. Test once then close.
- **Status:** NOT TESTED.

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
