# Strategy Ideas — Updated 2026-04-14

*2026-04-14 critique: Research is DONE. All non-trend strategies are GRAVEYARD'd. Turtle+Chandelier params are FROZEN. The remaining blockers are: (1) equity curve system has mixed execution logic — fix before next progress chart, (2) live testnet — blocked on API keys, (3) chop filter — ONE parameter test remaining. Stop writing new ideas. Ship what exists.*

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

## 9. Turtle Chop Filter (ATR Regime Entry Gate) — ⭐ MOST PROMISING
- **Concept:** Only enter Turtle when `atr(14) > median_atr(14, 252)`. Entry gate: volatility must be above its 1-year median. Filters low-vol chop (ranging markets with no clean breakouts). Standard practitioner's wisdom, never tested on crypto daily data.
- **Why:** Turtle takes almost every breakout signal (ATR entry filter hyperopt confirmed mult=0.0 is optimal — no filter). In W05 FTX-collapse, Base5 generated 14 trades with only 64% win rate. Some were false breakouts in ranging markets. A chop filter should reduce whipsaw losses without killing winning trades.
- **Why NOT another regime switcher:** Vol-rank conditional A/D×Turtle FAILED (60.5% pass vs component 72%/71%). But that was SWITCHING between strategies. This is a simple ENTRY FILTER on Turtle — different mechanism, same parameters, no strategy switching.
- **Status:** IN PROGRESS (2026-04-14). Walk-forward across 9 universes × 54 windows. Binary gate: only enter Turtle when ATR14 > 252-bar median ATR14. If pass rate > 92.6% baseline → add `USE_CHOP_FILTER=true` to production params. If fails → accept Turtle as-is.
- **Risk:** Could remove winning trades in early trend formation. Measure carefully.

---

## 10. True Chandelier Equity Curve (Fix Mixed Execution Systems)
- **Status:** IN PROGRESS (2026-04-14). The equity curve harness uses FIXED 21-bar hold for DDBudget AND for `turtle_chandelier_equity.csv`. The walk-forward uses Chandelier(28,2.0)+Turtle_ATR(25). **Result: the 519,288% return and 7.61 Sharpe in equity curves come from a DIFFERENT exit system than the validated walk-forward.** Fix: update `progress_equity_curves.rs` to use Chandelier dual-exit for Turtle+Chandelier entries. Regenerate CSV. One source of truth.
- **Priority:** HIGH — this is an integrity fix, not new research.

---

## 11. A/D Static Sleeve Allocation — LAST UNTESTED COMBINATION METHOD
- **Concept:** Turtle+Chandelier (80%) + A/D Dual-Hat (20%) with FIXED allocation, no regime switching. Simple voting (50/50) FAILED: combined pass rate 78% vs Turtle alone 87% and A/D alone 87%. Voting cancels when they disagree — the wrong combination method.
- **Why fixed allocation vs voting:** Correlation 0.11 (genuinely uncorrelated). Entry overlap only 1.3%. A/D wins crash windows (W01/W04/W06), Turtle wins bull windows. Fixed allocation lets both run independently without canceling.
- **Why not switching:** Vol-rank conditional A/D×Turtle already FAILED (60.5% pass — worse than either alone). Regime switching assumes strategies have cleanly separated niches — empirically wrong.
- **Status:** Not tested. Walk-forward: 9 universes × 6 windows, measure combined Sharpe vs Turtle alone. If Sharpe improves → add 20% A/D sleeve to production harness.
- **Risk:** A/D W05 failure (-40.4%) drags the sleeve in bear chop. But Turtle is already profitable in W05 (+55.5%) so the drag is capped.

## 12. Live Testnet 30-Day Gate
- **Concept:** After `live_turtle_chandelier.rs` connects to testnet: run 30 calendar days. At day 30: compare live Sharpe vs walk-forward Sharpe (6.73 gross / 5.25 fee-adj). If live Sharpe > 1.0 with real fills → promote to stage trading. If Sharpe < 0.5 → diagnose maker vs taker drift, execution lag, signal quality.
- **Why:** All fee models are theoretical. The ~70% maker fill assumption was measured on HISTORICAL data. Live market conditions (order book state, spread widening in crashes, fill rates) are unknown. 30 days is the minimum honest validation window.
- **Status:** BLOCKED on API keys. 30-day runtime minimum after testnet connection established.
- **Metric to track:** `live_vs_backtest_drift = (live_sharpe - walkforward_sharpe) / walkforward_sharpe`. Acceptable drift: -30% to +10%.

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

## What NOT to Research (Graveyard confirmed 2026-04-14)

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
