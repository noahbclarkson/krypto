# Strategy Ideas — Updated 2026-04-16

*2026-04-16 critique: Research CLOSED. Three honest execution tasks remain. All non-trend strategies GRAVEYARD'd. Turtle+Chandelier params FROZEN. **Stop auditing. Ship HALL_OF_FAME.md/GRAVEYARD.md, test A/D sleeve, re-run SOL slippage constraint, then live testnet.** The "Sharpe 5.0+" claim is dead — use 1.0-1.3 (daily equity).*

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

## 13. Cross-Market Equity Turtle (SPY/QQQ/GLD) — NEW
- **Concept:** Turtle+Chandelier works on SPY (Sharpe 0.87), GLD (0.87), QQQ (0.76). These are independently profitable on non-crypto markets. Build a `cross_market_turtle.rs` harness with full walk-forward validation on equities.
- **Why this matters (2026-04-16):** The crypto Sharpe (1.04 equity) may be inflated by crypto's high-vol regime. Equities give a cleaner signal-to-noise ratio. If SPY Turtle is independently profitable AND uncorrelated to crypto Turtle, combining them in a portfolio reduces drawdown without proportional return sacrifice.
- **Risk:** TLT/FXE/EWJ/ILF failed (Sharpe < 0.5). Only US equities and gold work. Universe must be curated.
- **Status:** NOT TESTED. Walk-forward needed: SPY/QQQ/GLD × 6 windows.

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

## 13. Cross-Market Equity Turtle (SPY/QQQ/GLD) — NEW 2026-04-16
- **Concept:** Turtle+Chandelier works on SPY (Sharpe 0.87), GLD (0.87), QQQ (0.76) on a simple point-in-time backtest. Build `cross_market_equity_walkforward.rs` with full 6-window OOS walk-forward on US equities and gold.
- **Why this matters:** The crypto equity Sharpe (1.04) may be inflated by crypto's high-vol regime. If equities pass OOS validation independently, the edge is proven market microstructure — not crypto survivorship bias. If they fail, we learn the edge is regime-dependent.
- **Risk:** TLT/FXE/EWJ/ILF failed (Sharpe < 0.5). Universe must be curated to US equities + gold only.
- **Acceptance:** ≥3/5 assets (SPY, QQQ, GLD, TLT, FXE) pass OOS → claim strengthened.
- **Status:** NOT TESTED. Walk-forward harness template exists (`turtle_chandelier_walkforward.rs`).

## 14. SOL Dollar-Sized Walk-Forward Re-test — NEW 2026-04-16
- **Concept:** $50K SOL cap is documented as a live-trading risk constraint only — the backtester uses % returns, not dollar sizing. Re-run NoDOGE walk-forward instrumenting per-trade notional and enforcing MAX_SOL=$50K.
- **Why:** SOL is the strongest paper-mode performer (+97%, 58.6% WR, 18% DD). If dollar-sizing cap materially degrades equity Sharpe (>10%), decision needed: cap harder, exclude SOL, or accept slippage as diversification cost.
- **Challenge:** Backtester returns % not $ — need to track per-trade notional separately or simulate dollar-sized fills.
- **Status:** NOT TESTED. Needs instrumented harness.

## 16. Multi-Timeframe Confirmation (4h→1d Entry Filter) — NEW
- **Concept:** Require 4h close > 4h SMA(21) as entry confirmation before daily Turtle signal. Not regime switching — just noise filter on entry.
- **Why promising:** 2026 YTD whipsaw is regime-inherent. ATR entry filter failed (trade-starving). 4h trend confirmation is qualitatively different — aligns daily breakout with short-term trend direction.
- **Risk:** ATR filter failed with same mechanism — could be trade-starving. Only build harness to know.
- **Status:** NOT TESTED. Needs `multitimeframe_turtle_walkforward.rs`.

## 17. Equity Portfolio Integration (SPY/QQQ/GLD + Crypto) — NEW
- **Concept:** Combined Turtle portfolio: BTC, ETH, SOL, SPY, QQQ, GLD. All have ≥53% OOS pass. Equities lower vol, negatively correlated in crashes.
- **Hypothesis:** Adding SPY/QQQ reduces MaxDD 10-15pp without proportional return reduction.
- **Why this matters:** Crypto equity Sharpe (~1.04) comparable to SPY (0.87). SPY W00 COVID: -36% → Turtle -2.4%. Same crisis protection mechanism.
- **Risk:** US equity weekends/gaps create signal artifacts. 24/7 crypto vs equity data alignment needs care.
- **Status:** NOT TESTED. Per-asset validated (SPY 88%, QQQ 76%, GLD 53%), combined portfolio walk-forward NOT tested.

## 18. Drawdown-Adaptive Signal Tightening — NEW
- **Concept:** When portfolio drawdown > 15%, raise entry threshold (EP=21→EP=25) + add 4h SMA confirmation. Revert when drawdown recovers. Changes SIGNAL QUALITY not position size.
- **Why different from failed overlays:** USDT hedge/BTC scalar/drawdown trigger all changed risk budget (position size). This changes entry quality — tighten requirements when already underwater.
- **Risk:** ATR entry filter (similar concept) destroyed pass rate. Cautious — one test then graveyard if it fails.
- **Status:** NOT TESTED. Needs dedicated walk-forward harness.
