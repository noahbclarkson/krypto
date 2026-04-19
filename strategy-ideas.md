# Strategy Ideas — Updated 2026-04-19 (Strategy Research & Critique)

*2026-04-19 critique: HALL_OF_FAME.md incorrectly claims cd=10 (live bot uses cd=0). daily_progress.csv polluted with false prototype Sharpes. Equity numbers inconsistent (673.5x vs claimed 1126x). Hyperopt redundancy loop confirmed. Research loop closed. Only live testnet matters.*

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
- **Updated (2026-04-18):** Extensive 31-value sweep {0..=30 step 1} across 9 universes found **cd=10** as the winner (65.6% pass, 0.242 Sharpe vs baseline 57.8% pass, 0.027 Sharpe). cd=3 won the coarse 6-value sweep on Base5 only. cd=10 is the 9-universe aggregate winner.
- **Live bot:** `src/live/bot.rs` uses cd=10 (commit 944bcd66).
- **Status:** ✅ CLOSED. cd=10 is production default. Live testnet is the only remaining validation — the effect is modest (noise-range) and regime-dependent.

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

## Post-Live-Testnet Concepts (For After 30-Day Live Validation)

These are ideas to research ONLY after live testnet confirms the maker-fill rate and signal quality in real market conditions.

### S1. Maker-Fill Adaptive Position Sizing
- **Concept:** After 30 days of live data: measure actual maker-fill rate per symbol. If maker-fill > 70% → full position size. If maker-fill < 50% → reduce position by 30%. The maker-fill rate is a market microstructure signal.
- **Status:** Unbuilt. Cannot test without live fill data.
- **Priority:** Medium (after live validation)

### S2. Live Slippage Tracker → Position Size Adjustment
- **Concept:** Track realized slippage per symbol in live trading. If SOL slippage consistently exceeds 2x model → reduce SOL position or cap at $25K. Create a live slippage dashboard.
- **Status:** Unbuilt. Requires live execution first.
- **Priority:** Medium

### S3. Multi-Strategy Live Sleeve (A/D as secondary)
- **Concept:** After live validates Turtle+Chandelier: add A/D Dual-Hat as a 20% sleeve for crash protection. A/D wins crash windows (W01/W04) historically.
- **Status:** Unbuilt. A/D walk-forward pass rate is only 52% standalone. Would need live validation of A/D signal quality before inclusion.
- **Priority:** Low (requires live A/D signal validation first)
