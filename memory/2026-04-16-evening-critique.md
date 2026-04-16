# Session Log — 2026-04-16 (Evening Critique)

## [16:50 UTC] Research & Critique Cycle — Krypto State of the Project

### Last 5 Commits (git log --oneline -8)

```
37e5c75 chore: daily progress tracking — CTREND 1438x, Turtle 271x, Sharpe stable
a58f0d1 hyperopt: DynamicTrend ema_fast sweep [1,100] walk-forward 252/252 across 4 universes
ca1a755 docs: strategy-ideas.md cleanup — remove duplicate entries, fix fabricated numbers
6dfe73e memory: 2026-04-16 — entry timing audit results logged, multi-TF rejected
2493fc8 feat: unranked vs ranked walk-forward — ranked decisively wins (6/6, Sharpe 2.54 vs 5/6, 0.97)
```

### Critique #1: CTREND Equity of 1438x is Potentially Inflated

**The problem:** `progress_equity_curves.rs` runs `StrategyKind::CTRend` with default `ema_fast=50`. The DynamicTrend hyperopt (a58f0d1) showed ema_fast=50 has pass rate 67.9% and **avg Sharpe -13.017** (negative). The winner ema_fast=60 has pass rate 85.7% and **avg Sharpe -25.163** (still negative, but fewer failures).

**The critical question:** Does the progress chart's CTREND equity (1438x, Sharpe 5.17) come from running the strategy on all bars (in-sample) vs properly computed OOS equity? If it's running on all bars with a parameter that has negative OOS Sharpe, the 1438x is meaningless — it's the equivalent of showing BollingerReversion's +5404 Sharpe before we killed it.

**The evidence:**
- `progress_equity_curves.rs` uses `build_symbol_plans()` which runs DynamicTrend::default() → ema_fast=50
- ema_fast=50: OOS walk-forward Sharpe = -13.017 (negative, 67.9% pass — below 70% threshold)
- The progress chart shows 1438x at day 2074 — this is the final equity of a strategy with negative OOS Sharpe
- The Sharpe 5.17 reported is likely computed from the in-sample equity curve, NOT from proper OOS windows

**The mechanism:** If the harness runs the strategy on every bar (in-sample), it's curve-fitting. The 1438x could be real if the strategy genuinely earns that in live trading, but we have NO OOS validation confirming ema_fast=50 or ema_fast=60 works out-of-sample on crypto.

**Verdict:** CTREND 1438x is unvalidated. Do NOT use it as evidence of edge until proper OOS walk-forward with a winning ema_fast parameter is run and equity is properly computed from OOS windows. The A/D strategy has similar risk — `build_symbol_plans()` likely uses A/D p=47 defaults, but p=2 was the bear-phase winner. Need to verify.

**Action:** Run `dynamic_trend_walkforward.rs` on Base5 with ema_fast=60 to get a proper OOS equity curve. If equity is flat or declining → GRAVEYARD. If equity shows genuine growth → update the progress chart with the correct OOS-derived equity.

---

### Critique #2: TURTLE_ATR_MULT=1.0 Listed in PLAN but 2.00 in Code

**The problem:** PLAN.md shows:
```
TURTLE_ATR_MULT = 1.0  (Turtle ATR stop — fine hyperopt 2026-04-16: +57% Sharpe vs coarse 2.0; Chandelier handles trend capture)
```

But `turtle_chandelier_walkforward.rs` line 31:
```rust
const TURTLE_ATR_MULT: f64 = 2.00; // hyperopt 2026-04-12...
```

**The history:**
- 2026-04-12 hyperopt: M=2.0 is optimal (Sharpe 6.17, 93% pass). M<2.0 degrades Sharpe. M>=2.5: Turtle ATR never fires first.
- The PLAN.md entry says "fine hyperopt 2026-04-16" but the hyperopt-2026-04-16-atr-period.md file is about ATR_PERIOD=24, not TURTLE_ATR_MULT.

**The confusion:** The TURTLE_ATR_MULT=1.0 appears to be a documentation error in PLAN.md. The actual code uses 2.00, and the ATR_PERIOD hyperopt (18-35 step=1, 9 universes, 54 windows) confirmed ATR=24 as the winner.

**Consequence:** This doesn't affect backtest results since the code is correct, but anyone reading PLAN.md would get the wrong production parameter.

**Action:** Fix PLAN.md to show `TURTLE_ATR_MULT = 2.0` with the correct hyperopt reference (2026-04-12-atr-mult.md).

---

### Critique #3: Progress Chart CTREND/AD Sharpe May Be In-Sample Artifact

**The evidence:** `progress_equity_curves.rs` computes daily equity for all strategies using `simulate_daily_equity()` which runs on the full combined history. The CTREND Sharpe of 5.17 is computed from this full-history equity curve, NOT from OOS windows.

**Contrast with Turtle:** Turtle's equity curve was FIXED in the last session (forward-fill bug). The fixed Turtle Sharpe is 0.97. The progress chart was correct for Turtle after the fix. But CTREND and other strategies may have similar bugs — we didn't check them.

**The deeper issue:** We validate strategies with walk-forward (OOS windows) and then display equity curves from running on all bars. The walk-forward tells us PASS RATE and AVG SHARPE per window. The equity curve tells us compounding. These are DIFFERENT objects. We don't have a proper OOS equity curve for CTREND — only an in-sample equity curve.

**Action:** Add a flag to `progress_equity_curves.rs` to compute equity ONLY from OOS bars for each strategy. This requires storing OOS windows per strategy, which the current harness may not support.

---

### The 3 Most Promising Unbuilt Ideas

**#1 — CTREND OOS Equity Validation**
- CTREND 1438x is currently unvalidated (likely in-sample)
- Run `dynamic_trend_walkforward.rs` with ema_fast=60 on Base5 to get proper OOS equity
- If OOS equity > 100x with pass rate > 70% → CTREND is a legitimate candidate alongside Turtle
- If OOS equity is flat → GRAVEYARD and remove from progress chart
- This is the single highest-value test we can run right now (no API keys needed)

**#2 — Drawdown-Adaptive Signal Tightening**
- New concept (entry 18 in strategy-ideas.md): when drawdown > 15%, raise entry threshold (EP=21→25)
- Different from all failed overlays: those changed POSITION SIZE, this changes SIGNAL QUALITY
- ATR entry filter failed (trade-starving), but drawdown-triggered quality tightening is conceptually different
- Need dedicated walk-forward harness; not in production yet
- Low risk to test — one harness, clear pass/fail metric

**#3 — Multi-Timeframe Confirmation (Re-evaluation)**
- 2026-04-16: 4h SMA(21) filter REJECTED (5/7 vs 6/7 baseline, Sharpe 1.86 vs 2.00)
- BUT: the test used ONLY 7 windows (one per symbol × 7 symbols). Small sample.
- The qualitative case is still compelling: 2026 YTD whipsaw (Turtle -22.7% vs BTC +12.7%) is real
- A different entry filter (not 4h SMA, perhaps trend强度指标 like ADX) might work
- Re-test with more windows (all 6 WF windows × 5 symbols = 30 windows) before fully rejecting

---

### The Biggest Blind Spot

**We have no live testnet data. Everything is backtest.**
- All our validation is OOS walk-forward, which is the right methodology
- But the gap between OOS walk-forward and live execution has never been measured
- We cannot know if maker fill rate, signal latency, or slippage model is correct without live data
- The SOL slippage at $100K is 3.7× our model — this could apply to other assets in ways we haven't measured

**The 2026 YTD underperformance (-22.7%) is the most honest signal we have** — it's live data (paper mode on historical data, but executed with current params). If the backtest was truly honest, 2026 YTD should be within the confidence interval. It wasn't.

**The CTREND/AD inflation risk:** If our progress chart shows 1438x for CTREND and that is in-sample, we're showing Noah a mirage. The real edge might be 10x or 0x — we don't know.

---

### Updated Metrics Assessment

| Strategy | Reported | Honest | Status |
|----------|----------|--------|--------|
| Turtle+Chandelier | 271x, Sharpe 0.97 | VALIDATED (OOS, forward-fill fixed) | ✅ HOF |
| CTREND | 1438x, Sharpe 5.17 | UNVALIDATED (likely in-sample) | ⚠️ NEEDS OOS TEST |
| A/D | 40x, Sharpe 3.62 | UNVALIDATED (default params, not optimal) | ⚠️ NEEDS OOS TEST |
| DDBudget | 58x, Sharpe 7.07 | MILESTONE-AGGREGATED (not daily equity) | ⚠️ NOT COMPARABLE |

**Never put CTREND 1438x in a Discord update until OOS validation is complete.**

---

### Priority Execution Tasks (Next Session)

**T1 — CTREND OOS Equity Validation (highest value, no blockers)**
- Run `dynamic_trend_walkforward.rs` with ema_fast=60 on Base5 (6 windows)
- Export OOS equity curve and compare to progress chart claim
- If OOS equity < 10x or Sharpe < 0.5 → GRAVEYARD, remove from progress chart
- If OOS equity validates → update progress chart with proper OOS equity

**T2 — Fix TURTLE_ATR_MULT in PLAN.md**
- Change `TURTLE_ATR_MULT = 1.0` → `TURTLE_ATR_MULT = 2.0`
- Update comment to reference correct hyperopt (2026-04-12-atr-mult.md)

**T3 — Unranked vs Ranked Walk-Forward (blocked on API keys, but can prepare)**
- Prepare harness with proper OOS equity for both variants
- Decision needed before live production switch
- Run locally first; result should be ready to apply when keys arrive

---

### What NOT to Do

- Don't build new strategy harnesses — we have enough strategies, none validated beyond Turtle
- Don't do more hyperopt on Turtle — params are frozen
- Don't claim CTREND 1438x until OOS validated
- Don't post equity charts with unvalidated strategy lines to Discord

---

### Discord Summary (for next cron session)

No Discord message this session. The critique will be committed and available for the next cron cycle to report.

The most important finding: **CTREND 1438x may be a mirage.** Next session should prioritize the OOS validation run before any further reporting.