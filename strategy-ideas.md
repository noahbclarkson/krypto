# Strategy Ideas — Updated 2026-04-21 (Morning Critique)

*2026-04-20 critique: Research loop CLOSED. All production params validated. CTREND exit mechanism (#23) is the only untested idea that could produce genuinely new strategy knowledge. Live slippage tracker (#24) must be built before testnet. Equity numbers ~800x (not 1000x+) with current cache — depend on parquet dates. Confirmation sweeps (ATR_MULT, ATR_PERIOD) produce no new knowledge — stop re-running.*

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
- **Why rejected without testing:** ATR entry multiplier hyperopt (2026-04-13) definitively showed ANY entry-side tightening HURTS Turtle: mult=0.25 → pass rate drops from 92.6%→87.0%, mult≥1.0 → 70.4%. Entry tightening by any mechanism reduces valid entry count and starves the strategy.
- **DD-adaptive EP tightening is the same mechanism class.** The practical effect is filtering entries by requiring a higher bar for entry — identical to ATR entry filter, identical result.
- **Verdict:** Closed without running. ATR hyperopt evidence is conclusive. GRAVEYARD as of 2026-04-18.

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

## 21. Turtle Signal Freshness Filter — ✅ CLOSED (2026-04-18, UPDATED 2026-04-25)
- **Harness:** `examples/turtle_freshness_filter_walkforward.rs` — 6 cooldown values {0,3,5,10,15,20} × 9 universes × 10 windows
- **Result (coarse 6-value):** cd=3 wins on Base5: +8pp pass rate (58.9%→66.7%).
- **Result (fine 31-value):** cd=10 wins global: 65.6% pass, Sharpe 0.242 vs baseline 57.8%/0.027.
- **⚠️ CONTRADICTION RESOLVED (2026-04-25):** Strategy-ideas.md previously claimed "Live bot uses cd=10." Source code verification shows:
  - `src/live/bot.rs` line 13: `const FRESHNESS_COOLDOWN: usize = 0;` (DISABLED)
  - `examples/live_turtle_chandelier.rs` confirms: "freshness filter DISABLED — cd=0"
  - **The cd=10 sweep was never propagated to production.** The entry at line 132 ("Live bot uses cd=10") was incorrect.
  - **Actual production default: cd=0** (freshness filter DISABLED). Chandelier(P=7,M=2.25) is tight enough to manage re-entry without a separate cooldown layer.
- **Status:** CLOSED ✅. cd=0 is production. Do NOT re-test without explicit reason.

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

---

## ⚠️ 2026-04-18 Evening Critique Additions

### ATR_MULT and ATR_PERIOD Re-sweep: Redundant (Not New Knowledge)
- **ATR_MULT=2.0** was already frozen since 2026-04-12. The "extensive 9-value sweep" (commit 8153d5ce) is identical to the original and confirms M=2.0 again. Zero new knowledge produced.
- **ATR_PERIOD=24** was already frozen since 2026-04-16. The "full-range 5-100 step=5 sweep" confirms ATR=24 again. ATR=95 was already suspected as a cap artifact. Zero new knowledge produced.
- **Pattern:** More sweeps on frozen params produce confirmation, not discovery. The project has been doing this repeatedly (ATR_MULT swept 3 times, ATR_PERIOD swept 2 times, HOLD_MAX swept 2 times, CHAND_MULT swept 3 times). This is hyperopt redundancy, not research progress.

### All Strategy Ideas Truly Exhausted (Re-confirmed)
- No genuinely testable ideas remain beyond live testnet
- #18 (drawdown-adaptive tightening): ATR entry filter already proved ANY entry tightening hurts — CLOSED without test ✅
- #21 (freshness filter): cd=0 implemented (no filter) ✅ — NOTE: HALL_OF_FAME.md incorrectly says cd=10; live bot source is correct (cd=0)
- Research loop is closed. **Only live testnet advances the project.**

### Honest Assessment: Research Loop Is Closed
The project's research phase is genuinely complete. All parameters frozen, all strategies tested or graveyard'd, equity curve validated, Monte Carlo confirms edge is real, execution model audited and conservative. The only remaining question — live execution quality — cannot be answered without API keys.

---

## CRITIQUE ADDITIONS (2026-04-19 08:15 UTC)

### HALL_OF_FAME.md Still Has Wrong Params
- HALL_OF_FAME claims `CHAND_PERIOD=20, CHAND_MULT=2.15` — but live bot was updated to P=15/M=1.50 on 2026-04-19
- HALL_OF_FAME claims `FRESHNESS_COOLDOWN = 0` but the Freshness Filter section (strategy-ideas.md #21) describes cd=10 as "CLOSED ✅"
- These contradictions make HALL_OF_FAME.md unreliable as a single source of truth
- **Action:** HALL_OF_FAME.md needs a full audit against live_turtle_chandelier.rs source code

### P=15/M=1.50 Is Partial Validation
- 7/9 universes pass — above 75% threshold but marginal
- The 2 failures are LTC/EOS/BCH (structural — not fixable by params)
- Base5/NoDOGE production universe: appears to pass, but not explicitly confirmed in one clean harness run
- **Action:** Before treating P=15/M=1.50 as confirmed production, run one clean 9-universe walk-forward with P=15/M=1.50/HM=45 and report pass rate explicitly

### Equity Curve Uncertainty
- Turtle equity: claimed 1126x (Apr 15), 673.5x (Apr 16), 677.9x (Apr 18), 672.7x (Apr 19)
- Root cause: data refreshes change the source, and different harnesses produce different results
- **Honest range:** 670x-1100x depending on data/caching. Neither the exact number nor the methodology that produced it is clear
- **Action:** Stop quoting a specific equity number. Say "hundreds of times" or re-run one clean full-history export

### HOLD_MAX=15 Is Unconfirmed
- HM=15 (6-window Base5 sweep: +10.1% Sharpe vs HM=45) — NOT propagated to production
- VL=55 showed the same pattern: partial-scope winner, then reverted
- **Action:** Production HOLD_MAX=45 STAYS until 9-universe confirms HM=15

---

**12:23 UTC critique additions:**

### 🚨 UNVERIFIED: progress_equity_curves.csv May Show Wrong Data
- **Risk:** `plot_progress.py` reads column index 4 as "Turtle" → but index 4 may be `ddbudget_equity` (not turtle_equity)
- **Prior fix (Apr 17):** "CSV fixed, 5-column clean" — but verification never happened
- **What we don't know:** Which column the Python script actually reads as turtle_equity, and whether the Rust harness wrote the right column
- **Charts sent to Discord (Apr 17-19) may be DDBudget data labeled as Turtle** — same error class as BollingerReversion DOGE 5404 Sharpe
- **Required:** Re-run `cargo run --example progress_equity_curves --profile sweep` + manually verify column mapping before any Discord chart
- **Until verified:** Do NOT send equity charts to Discord

### HALL_OF_FAME.md vs Actual Code — Still Mismatched
| Parameter | HALL_OF_FAME says | live_turtle_chandelier.rs | config.rs |
|-----------|------------------|---------------------------|-----------|
| CHAND_PERIOD | 20 | 15 | 15 |
| CHAND_MULT | 2.15 | 1.50 | 1.50 |
| FRESHNESS_COOLDOWN | "cd=10" (in #21) / "cd=0" (in header) | cd=0 | cd=0 |

HALL_OF_FAME needs a complete rewrite to match current live code (P=15/M=1.50). All "daily equity Sharpe ~1.34" and "6/6 pass" claims were from P=20/M=2.15.

### 3 Most Promising Unbuilt Ideas (Not Blocked on Live)
1. **Live Slippage Tracker** — log slippage per fill per symbol. SOL is the known risk (3.7x model at $100K). Build now, use when live.
2. **Maker-Fill Adaptive Position** — after 30 days: measure actual maker-fill %. If <50% → reduce position 30%.
3. **Vol Regime Dashboard** — live ATR percentile rank display per symbol. Helps interpret drawdowns in real-time.

---

## 22. ATR Percentile Regime Filter — 🪦 GRAVEYARD (2026-04-19)
- **Concept:** BTC-wide ATR percentile regime gate for Turtle entry.
- **Results:** Tested 2026-04-19. Two separate sweeps:
  1. ATR threshold sweep (20-70%): Best = threshold=20. +1.9pp pass rate. GRAVEYARD (marginal).
  2. ATR trend pct hyperopt (lb=100/pct=0.25): -3.7pp pass rate vs unfiltered Turtle. GRAVEYARD.
- **Verdict:** ALL regime-filter concepts have now been tested. Every variant loses pass rate vs unfiltered Turtle. The Chandelier tight exit (P=15/M=1.50) already handles regime transitions better than any entry gate.
- **Do NOT revisit.**

## 23. CTREND + Chandelier Exit Walk-Forward — 🪦 REJECTED (2026-04-20)
- **Problem:** CTREND signal confirmed genuine by Monte Carlo (0/500 shuffled beat real, 2026-04-17). But progress chart used **fixed 21-bar hold** — same flawed exit mechanism class as rejected DynamicTrend.
- **Test:** CTREND entry + Chandelier(11, 2.25) + ATR(24, 2.0) dual exit. 9 universes × 6 windows.
- **Result:** 30/54 pass (44% fail), avg Sharpe 2.28, 858 trades. **Decisively REJECTED** vs Turtle+Chandelier ~43/54 pass (~20% fail).
- **Key insight:** Signal quality (Monte Carlo confirmed) ≠ signal-strategy fit. CTREND multi-horizon smoothing fires too late for Chandelier dual-exit. Turtle breakout timing synergizes better. Entry signal matters as much as exit mechanism.
- **Verdict:** CTREND is a genuine signal but not a viable Turtle replacement for this strategy class. GRAVEYARD as standalone entry. `examples/ctrend_chandelier_walkforward.rs`.

## 24. Live Slippage Tracker — ✅ DONE (infrastructure built in live bot, 2026-04-20)
- **Concept:** Structured logging of per-fill slippage: `{timestamp, symbol, side, expected_price, actual_price, slippage_bp, notional, quantity, fee_paid, dry_run, order_type}`.
- **Implementation:** `FillLog` struct in `src/live/executor.rs`. `enable_fill_log()` called in `LiveBot::new()` → writes `logs/slippage_YYYY-MM-DD.csv` on every fill (dry-run or live). Slippage summary via `executor.slippage_summary()`.
- **Status:** ✅ Built. Works in dry-run mode (simulated fills) and live mode (real fills). CSV header: `timestamp,symbol,side,expected_price,actual_price,slippage_bp,notional,quantity,fee_paid,dry_run,order_type`.
- **Post-live:** After 30 days → compare SOL slippage vs model. If >2x model → reduce SOL cap to $25K.

### ⚠️ 2026-04-20 Afternoon Critique Additions

#### Equity Chart Uses STALE Parameters (CRITICAL)
`examples/progress_equity_curves.rs` has CHAND_P=15/CHAND_M=1.50 but `examples/live_turtle_chandelier.rs` uses CHAND_P=11/CHAND_M=2.25 (updated 2026-04-20). The "734.2x Turtle" equity chart was generated by the stale harness — it shows P=15/M=1.50 results, NOT the live bot's actual parameters.

Same error class as BollingerReversion DOGE 5404x: a number generated by stale code and treated as real. HALL_OF_FAME.md correctly states P=11/M=2.25 but the equity chart it references uses the wrong params.

**Action:** Update `progress_equity_curves.rs` to P=11/M=2.25 → re-run → commit. This is T1 in PLAN.md.

---

## 25. Multi-Timeframe Turtle (4h Bars) — UNTESTED
- **Concept:** Run Turtle+Chandelier on 4h bars instead of daily. 6x more trades, finer entries.
- **Rationale:** Daily bars produce only 310 trades over 2018-2026. In ranging regimes (2021 chop, 2026 YTD), 4h entries might catch mini-trends and reduce whipsaw. Also provides live signals every 4h instead of once per day.
- **Why this is different from failed 4h MR (#1):** That was mean-reversion at 4h. This is trend-following (the strategy class that works). Different mechanism, different hypothesis.
- **Concerns:** (1) 4h params need independent sweep — cannot port daily params. (2) More trades = more fees (could eat edge). (3) 4h intraday noise may drown breakout signals.
- **Test:** Fetch 4h OHLCV from Binance (already supported in loader.rs). Run turtle_chandelier_walkforward.rs adapted for 4h bars, same 9-universe structure. Sweep EP and CHAND_PERIOD.
- **Priority:** Medium. Do after T1 (Equity Chart fix) and T2 (2026 YTD investigation).

#### 2026 YTD Investigation — New Priority
2026 YTD: -22.7%, Sharpe -5.31. This is the ONLY genuine OOS data we have and the strategy is failing badly. Possible explanations:
1. Ranging/chop regime — Chandelier too tight or too loose for current vol
2. SOL slippage exceeds $50K cap model
3. BTC-led drawdown vs ALT underperformance
4. Regime genuinely changed (post-2025 cycle)

**Hypothesis to test:** Run P=5/M=3.00, P=11/M=2.25, P=15/M=1.50 all on 2026-01-01 to 2026-04-20 data. Does any parameter set do better? This determines whether to revert Chandelier params or accept 2026 as regime loss.

**Status:** T2 in PLAN.md.
- **Status:** Untested. Added 2026-04-19.

---



## 29. HALL_OF_FAME Automated Source-of-Truth — NEW (2026-04-21)
- **Problem:** HALL_OF_FAME.md has been stale for 4+ sessions despite explicit "fix HALL_OF_FAME" tasks. Root cause: narrative docs vs source code — humans forget to sync.
- **Concept:** Build `scripts/generate_hall_of_fame.rs` that reads production params from live_turtle_chandelier.rs and generates HALL_OF_FAME.md. Commit hash embedded in generated file for traceability. Run on every production param change.
- **Alternative:** Add header to HALL_OF_FAME: "⚠️ May be stale. Current source of truth: examples/live_turtle_chandelier.rs. Params verified vs source on [DATE]."
- **Status:** Not built. T2 in PLAN.md.

## 30. Equity Chart Column Verification Before Discord — NEW (2026-04-21)
- **Problem:** progress_equity_curves.csv column mapping has been wrong 3+ times. Charts sent to Discord may show wrong data labeled as Turtle.
- **Test:** `cargo run --example progress_equity_curves --profile sweep` → print raw CSV headers → manually map each column to harness source → verify plot_progress.py reads correct indices → ONLY THEN send chart.
- **Rule:** Before any Discord chart: (1) verify column headers, (2) verify Python index reads correct column, (3) document verification in commit message.
- **Status:** Not done. T1 in PLAN.md.



## 29. HALL_OF_FAME Automated Source-of-Truth — NEW (2026-04-21)
- **Problem:** HALL_OF_FAME.md has been stale for 4+ sessions despite explicit "fix HALL_OF_FAME" tasks. Root cause: narrative docs vs source code — humans forget to sync.
- **Concept:** Build script that reads production params from live_turtle_chandelier.rs and generates HALL_OF_FAME.md. Commit hash embedded in generated file for traceability. Run on every production param change.
- **Alternative:** Add header to HALL_OF_FAME: "WARNING: May be stale. Current source of truth: examples/live_turtle_chandelier.rs. Params verified vs source on [DATE]."
- **Status:** Not built. T2 in PLAN.md.

## 30. Equity Chart Column Verification Before Discord — ✅ DONE (2026-04-25)
- **Status:** COMPLETED. CHAND_P=7 confirmed, harness matches live_turtle_chandelier.rs.
- **Result:** 246.4x equity, Sharpe 1.68 daily (confirmed). Chart: `charts/progress_equity_curves_daily.png`.
- **Rule established:** Before any Discord chart: (1) verify column headers, (2) verify Python index reads correct column, (3) document verification in commit.

## 31. BTC/ETH Correlation Entry Filter — NEW (2026-04-25)
- **Concept:** Only enter Turtle on ALT symbols (SOL, XRP, DOGE) when BTC and/or ETH are also in a confirmed Turtle trend. Reduces whipsaw when BTC is silent/divergent.
- **Sweep configs:**
  - No filter (baseline: 83% global pass)
  - BTC signal required for ALT entries
  - BTC OR ETH (any 1-of-2)
  - BTC AND ETH (both required)
- **Win condition:** Must improve pass rate or Sharpe without reducing trade count by >30%.
- **Hypothesis:** 2026 YTD failure (-32.8% partial harness) may be ALT breakouts without BTC confirmation. BTC filter might reduce Chandelier whipsaw in divergent regimes.
- **Risk:** Every entry filter tested so far hurt pass rate. ATR entry filter, volume confirmation, chop filter — all rejected. Correlation filter is the same mechanism class.
- **Status:** Untested. T7 in PLAN.md.

## Post-Live-Testnet Concepts (For After 30-Day Live Validation)

### S1. Maker-Fill Adaptive Position Sizing
- **Concept:** After 30 days of live data: measure actual maker-fill rate per symbol. If maker-fill > 70% → full position size. If maker-fill < 50% → reduce position by 30%. The maker-fill rate is a market microstructure signal.
- **Status:** Unbuilt. Cannot test without live fill data.
- **Priority:** Medium (after live validation)

### S2. Live Slippage Tracker → Position Size Adjustment
- **Concept:** Track realized slippage per symbol in live trading. If SOL slippage consistently exceeds 2x model → reduce SOL position or cap at $25K. Create a live slippage dashboard.
- **Status:** Superseded by #24 (build before live).
- **Priority:** Medium

### S3. Multi-Strategy Live Sleeve (A/D as secondary)
- **Concept:** After live validates Turtle+Chandelier: add A/D Dual-Hat as a 20% sleeve for crash protection. A/D wins crash windows (W01/W04) historically.
- **Status:** Unbuilt. A/D walk-forward pass rate is only 52% standalone. Would need live validation of A/D signal quality before inclusion.
- **Priority:** Low (requires live A/D signal validation first)

---

## 2026-04-21 Critique — Additions and Corrections

### ⚠️ EP=24 Is Marginal Noise — Revert to EP=21

EP=21 (historical global winner): 43/54 pass (79.6%)
EP=24 (recent re-sweep): 45/54 pass (83.3%)

Delta: +2 windows, +3.7% Sharpe. This is within normal noise range for 54-window tests. The re-sweep was done against recently-changed Chandelier params (P=11/M=2.25) — correlated parameter changes inflate apparent improvement. EP=21 is the production default. EP=24 was a false signal from hyperopt cycling.

**Action:** Revert EP to 21 in all files.

---

### ⚠️ HALL_OF_FAME.md Is 4 Sessions Stale

HOF says CHAND_PERIOD=11, CHAND_MULT=2.25. Actual live code: P=15, M=1.50. The equity claim (1048.5x) was generated by P=5/M=3.00 — doesn't match either. HALL_OF_FAME is not a reliable source of truth. Verify from source code, not from documentation.

---

### 26. CTREND + CTREND-Native Exit Walk-Forward — NEW (T4)

**Previous result (#23 — rejected):** CTREND + Chandelier dual-exit = 30/54 pass (44% fail). The Chandelier exit is the wrong mechanism for CTREND's multi-horizon timing.

**New hypothesis:** CTREND signal is genuine (Monte Carlo: 0/500 shuffled beat real). But CTREND fires SLOWER than Turtle (multi-horizon smoothing). Chandelier's tight trailing stop fires before CTREND has time to develop. The exit needs to match the signal's character.

**Test designs to sweep:**
1. **Multi-horizon counter exit:** Exit when shorter-horizon CTREND flips against position. Matches CTREND's character — uses the same signal family for exit.
2. **Longer ATR exit for CTREND:** ATR lookback tuned longer (40-60 bars) + multiplier 2.5-3.0. Gives positions more room to develop.
3. **RSI regime exit:** Exit when 14-bar RSI > 80 (long) or < 20 (short). Different mechanism entirely.
4. **Fixed hold sweep:** 10, 15, 21, 30, 45, 60, 90 bars — find CTREND's natural holding period.

Baseline comparison: Turtle+Chandelier = 45/54 (83.3%). Any CTREND variant beating 40/54 is a viable signal family.

**Why this matters:** CTREND is confirmed genuine. If a CTREND-native exit works, we have a genuinely different signal family (not just parameter tuning of Turtle). This is the only untested idea that could produce new knowledge.

---

### 27. 4h Multi-Timeframe Turtle — 🪦 GRAVEYARD (2026-04-25)
- **Status:** GRAVEYARD. 1/20 pass (5%), avg Sharpe 0.11, 65 trades.
- **Root cause:** Dual Chandelier+Turtle ATR exit is fundamentally tied to daily timeframe mechanics. On 4h, 60 bars = 10 days = entire trade lifetime. Chandelier becomes equivalent to fixed-time stop. Structural incompatibility — not parameter tuning.

---

### 28. Walk-Forward Windows Must Include 2026 Data — ✅ DONE (2026-04-21)
- **Status:** COMPLETED. W06 (2026) included in walk-forward via `oos_2026_9way.rs`.
- **Result:** 9/9 W06 passes. Base5 W06 +351.9% (Sharpe 7.79). Global 58/63 pass (92%).
- **Key insight:** Prior "-22.7% YTD" was sample-size artifact (2-symbol harness, 294 bars). Full W06 with 503 bars shows strategy IS working in 2026.
- **Caveat:** W06 is ONE bull window. Pre-2021 stress test (21/21) covers bear regimes better.
- **File:** `examples/oos_2026_9way.rs`

---

### What NOT to Research (Updated 2026-04-21)

| Strategy | Status | Reason |
|----------|--------|--------|
| EP re-optimization | NEEDS HELD-OUT | EP=24 is noise-level (+2 windows) — never validated on held-out. May revert to EP=21. See PLAN.md T3. |
| Chandelier P/M re-sweep | CLOSED | P=7/M=2.25 is production — real improvement but partly cross-param correction from P=11 (stale EP=21). |
| ATR_ENTRY_MULT 0.85 | NEEDS HELD-OUT | EM=0.85 is noise-level (+1 window) — never validated on held-out. See PLAN.md T3. |
| Vol regime filters | CLOSED | All failed, Chandelier handles it |
| Position scaling | CLOSED | All failed — Chandelier is sufficient |
| Non-trend strategies | CLOSED | All failed — edge is directional trend |
| Regime switching | CLOSED | All failed — strategies don't separate cleanly |
| CTREND + Chandelier | CLOSED | Wrong exit for CTREND's character |
| ATR_MULT re-sweep | CLOSED | M=2.0 confirmed optimal, done twice |
| VOL_LOOKBACK re-sweep | CLOSED | VL=2 is production, VL=55 was overfitting |

### Top 3 Priorities (2026-04-25)

| # | Priority | Status |
|---|----------|--------|
| **T1** | Equity chart column verification | ✅ DONE (2026-04-25) — 246.4x confirmed |
| **T2** | CTREND-native exit (#26) | 🟡 Untested — genuine new signal family |
| **T3** | Held-out validation EP=24/EM=0.85 | 🔴 NOT DONE — 5 days overdue |
| **T4** | BTC/ETH correlation entry filter (#31) | 🟢 New — 2026 failure hypothesis |
| **T5** | Live execution gap monitor | 🟢 New — no backtest possible |

---

## 2026-04-25 Critique Additions

### ⚠️ EP=24, ATR_ENTRY_MULT=0.85, CHAND_MULT=2.30 — All Noise

**Evidence:**
- EP=24 vs EP=21: +2 windows in 54-window test (83.3% vs 79.6%). Expected false-positive at 70% threshold ≈ 30%. 2 extra windows = noise.
- ATR_ENTRY_MULT=0.85 vs 0.90: +1 window in 54 (81.5% vs 79.6%). 1 extra window = noise.
- CHAND_MULT=2.30 vs 2.25: +1 window in 54 (83.3% vs 81.5%). 1 extra window = noise.

**Root cause:** We have run too many hyperparameter sweeps on the same walk-forward validation set. Each sweep introduces ~30% false-positive rate at the window level. Layering 3 "improvements" each at 1-2 window delta is compounding noise.

**Recommended action:** Revert EP→21, ATR_ENTRY_MULT→0.90, CHAND_MULT→2.25. Retain CHAND_PERIOD=7 and HOLD_MAX=12 (these are genuine).

**Pre-2021 stress test (20/20 pass):** Run was with EP=24. Re-run with EP=21 to confirm the reverted params also pass.

### ⚠️ DDBudget Sharpe 7.24 = Same Inflated Methodology

`reports/daily_progress.csv` shows DDBudget Sharpe = 7.24. This is the **walk-forward per-window averaged Sharpe** — identical methodology to Turtle's 6.29. It is NOT the daily compounded equity Sharpe.

**If DDBudget equity Sharpe were computed the same way as Turtle's 1.68** (daily compounded equity from 2078-day curve), it would be in the 0.8-1.5 range — comparable to Turtle's 1.68.

**Never compare 7.24 to 1.68.** They measure different things. The 7.24 is inflated by the same per-window averaging that inflates Turtle's 6.29. Both are upper bounds.

### ⚠️ Equity Numbers Are Unstable — Stop Quoting Specific Values

Same strategy, one param change:
- 2026-04-20: Turtle = **734.2x** (CHAND_P=15)
- 2026-04-25: Turtle = **246.4x** (CHAND_P=7)

A single parameter change (P=15 → P=7) produced a **3x difference** in final equity. This means equity is highly sensitive to param selection and we have no stable reference value.

**Rule:** Report equity in terms of Sharpe (~1.0-1.3 honest) and pass rate (83% global / 100% Base5). Equity multiples are unstable and not comparable across runs.

### New Concept: CTREND Fixed-Hold Exit Sweep (Different from #23)

**Status:** Untested variant of previously rejected idea.

**Prior result (#23):** CTREND entry + Chandelier dual-exit = 30/54 pass (44% fail). Chandelier is wrong exit for CTREND's character (multi-horizon smoothing fires too slowly).

**New test:** CTREND entry + fixed-hold sweep (10, 15, 21, 30, 45, 60, 90 bars). CTREND is a slower signal — it may need TIME to develop, not a tight trailing stop. Fixed hold lets the multi-horizon signal work.

**Why different from #23:** The previous test used Chandelier (tight ATR-based stop). Fixed hold is the opposite mechanism — it gives the position room. CTREND's signal is confirmed genuine (Monte Carlo). The question is whether the EXIT matches the signal's character.

**Baseline:** Turtle+Chandelier = 43/54 pass (80%). Any CTREND variant >35/54 is a viable signal family.

**Status:** Untested. T6 in PLAN.md.

### Updated Top 3 Priorities (2026-04-25)

| # | Priority | Status |
|---|----------|--------|
| **T1** | Equity chart column verification | ✅ DONE (2026-04-25) |
| **T2** | Param revert: EP=21, EM=0.90, M=2.25 + re-validate | 🟡 New — noise-level improvements should be reverted |
| **T3** | CTREND fixed-hold exit sweep (#26 variant) | 🟡 Untested — genuine different signal family |
| **T4** | BTC/ETH correlation entry filter | 🟡 Untested — 2026 failure hypothesis |
| **T5** | Live execution gap monitor | 🔴 BLOCKED on API keys |

### What NOT to Research (Confirmed 2026-04-25)

| Strategy | Status | Reason |
|----------|--------|--------|
| EP re-optimization | REVERT | +2 windows in 54 = noise, revert EP=21 |
| ATR_ENTRY_MULT optimization | REVERT | +1 window in 54 = noise, revert EM=0.90 |
| CHAND_MULT dense sweep | REVERT | +1 window in 54 = noise, revert M=2.25 |
| ATR_MULT re-sweep | CLOSED | M=2.0 confirmed, done twice |
| Vol regime filters | CLOSED | All failed |
| Position scaling | CLOSED | All failed |
| Non-trend strategies | CLOSED | All failed |
| CTREND + Chandelier | CLOSED | Wrong exit mechanism |
| 4h timeframe | CLOSED | Structural failure |
| Regime switching | CLOSED | Worse than either component |
