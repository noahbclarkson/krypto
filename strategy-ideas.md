# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-04 20:05 UTC. Critique cycle complete. AP=63 anti-overfit violation identified — config.rs updated without held-out validation (same EP=24 pattern). T59 and T60 still unbuilt (second session overdue). New concepts added: weekend filter, per-bar PnL attribution, aggTrades order flow.*

---

## Critical Alert: ATR_RANK=24 GRAVEYARD'd

**ATR_RANK=24 REJECTED via held-out validation (a324a0fa, 2026-05-04).**
- Pre-2021 held-out: T=24 → **10/22 pass, Sharpe -0.964** vs T=5 → **14/22 pass, Sharpe +0.664**
- Same EP=24 pattern: 3rd sequential optimization on same harness → failed held-out
- Production: **ATR_RANK_THRESHOLD = 5.0** (reverted from 24)

## Critical Alert: AP=63 ANTI-OVERFIT VIOLATION

**config.rs was updated to `REGIME_ATR_PERIOD = 63` without held-out validation.**
- `snapshots/ap_hyperopt.md` own "Next Steps" says: "Held-out validation required before updating config.rs"
- But config.rs was already updated to AP=63
- Pattern: AP=63 won by +1 window (+1.197 Sharpe, marginal) on the same OOS harness
- This is EXACTLY the EP=24 failure mode: marginal win → promoted to production → failed held-out
- **Action:** Run held-out validation (T62) before trusting AP=63. Do not let it sit unvalidated in production.

## Critical Alert: LOB NOBI Data Is Missing

**T49 is back to zero. `data/cache/lob_nobi/` is EMPTY.**
- Prior sessions claimed "one harness run away" — this was WRONG
- `examples/depth_imbalance_pipeline.rs` is a 6-line stub (hardcoded print, never connected to data)
- Requires: daemon → data persistence → harness → validation (multi-session project)

## Resolved

- **VOL_LOOKBACK drift:** config.rs and live_compatible_wf.rs both now use VL=96 ✅
- **Short-side sleeve:** GRAVEYARD'd (37.5% pass vs 69.1% guardrail) ✅
- **SIZE_MULT:** INERT — cosmetic risk knob only ✅

---

## Top 3 Most Promising Unbuilt Ideas

### T59: Turtle-Only Daily Equity Curve (IMMEDIATE)
**Status:** UNBUILT.
- **Problem:** The live bot runs Turtle-only + ATR_RANK=5. `progress_equity_curves.rs` runs Dual Exit (Chandelier+Turtle). We have NO compounded daily equity curve for the live strategy.
- **Action:** Build a Turtle-only mode for `progress_equity_curves.rs` that exactly matches `src/live/bot.rs` (using `check_turtle_exit` logic).
- **Why:** Every reporting metric for production is currently using the wrong strategy.

### T60: Per-Year Performance Decomposition (IMMEDIATE)
**Status:** UNBUILT.
- **Problem:** Unknown bull market bias. Walk-forward Sharpe averages per-window metrics, masking multi-year drawdowns.
- **Action:** Decompose the (new) Turtle-only daily equity curve by calendar year (2020-2026).
- **Output:** Report Sharpe, MaxDD, Return, and Trade Count per year.
- **Why:** If the edge only exists in 2020-2021, the strategy is not robust for 2026.

### T61: Binance aggTrades Order Flow Signal (HIGH — 1-2 Sessions)
**Status:** NEW CONCEPT.
- **Problem:** LOB NOBI (T55) is 6+ weeks away from having enough data.
- **Action:** Download historical `aggTrades` from data.binance.vision. Build rolling 5-min buy/sell imbalance. Test as entry confirmation gate.
- **Why:** Genuinely novel microstructure edge that doesn't require a 6-week collection period.

### T62: Weekend Liquidity Filter (NEW — 1 session)
**Status:** NEW CONCEPT.
- **Hypothesis:** Crypto weekend volume is 30-50% lower. Breakout breakouts on Saturday/Sunday bars may be structurally less reliable due to thinner books and higher slippage.
- **Action:** Add day-of-week filter to Turtle entries. Skip entries on Sat/Sun (or reduce position size). Test on existing daily data immediately.
- **Why:** Immediately testable, no new data required, low implementation complexity.

### T63: Per-Bar PnL Attribution (NEW — 1 session)
**Status:** NEW CONCEPT.
- **Hypothesis:** Turtle trend-following should show: few large wins, many small losses. Need to confirm this distribution.
- **Action:** Using Turtle-only daily equity curve (T59), decompose returns by trade outcome: (a) winners vs losers distribution, (b) fee cost as % of gross, (c) largest drawdown periods.
- **Why:** Identifies if the edge is from rare large trends (robust) or many small edges (fragile to outlier events). Answers "how much of equity is from 2020-2021 mega-bull vs distributed across years?".

---

## Infrastructure / Trust (Must Fix)
- [x] **T54**: ATR_RANK=5 equity run — UNBUILT, production flying blind (10 min fix)
- [ ] **T53**: Mock exchange — UNBUILT, 5+ weeks overdue
- [ ] **T55**: LOB NOBI data collection — DATA MISSING, multi-session

### Bypass Blocker: Local Mock Exchange
**Status:** UNBUILT. 5+ weeks overdue.
- Noah's API keys blocking testnet
- Build Rust HTTP/WS server that mocks binance-rs-async endpoints (order, account, market data)
- Seed with historical 1m parquet data from `data/cache/`
- Simulate fills (limit order at close → maker fill), slippage, order state machine
- Test `src/live/bot.rs` end-to-end without Binance credentials
- **Why:** Highest leverage. Execution testing unblocks all downstream validation.

---

## Tested and Rejected (Do Not Revisit Without New Mechanism)

| Strategy | Result | Key Reason |
|----------|--------|------------|
| ATR_RANK=24 | **GRAVEYARD** | **Held-out: 10/22 pass/-0.964 Sharpe vs T=5 14/22/+0.664. Same-harness artifact.** |
| ATR_ENTRY_MULT (all) | REJECTED | EM=0.00 wins definitively |
| Short-side sleeve | **GRAVEYARD** | 37.5% pass vs 69.1% guardrail. Signal too sparse (175 trades/253 windows). |
| SIZE_MULT overlay | INERT | Return/DD scale linearly with M — pure risk knob, no alpha. M=0.70 confirmed. |
| AP=64 | REJECTED | Sequential optimization on same harness. AP=12 confirmed as default. |
| VL=96 | ⚠️ RECONCILED | Both config.rs and live_compatible_wf.rs now use 96. |
| EP=24 | REVERTED | Held-out: 25/29 vs EP=21 27/29. Same-harness artifact. |
| REGIME_ATR_PERIOD=64 | REJECTED | Sequential optimization on same harness (EP=24 pattern). |
| VOL_LOOKBACK hyperopts [1..200] | CONFIRMED NULL | VL=96 plateau vs VL=8 — reconciled |
| ATR_EMA [1..200] | CONFIRMED NULL | No improvement anywhere in range |
| FRESHNESS_COOLDOWN [0..70] | CONFIRMED NULL | cd=0 optimal |
| HOLD_MAX [1..100] | CONFIRMED NULL | HM=12 optimal |
| CHAND_MULT dense [1.5..5.0] | CONFIRMED NULL | M=2.30 optimal |
| ATR_ENTRY_MULT [0.00..2.00] step 0.01 | CONFIRMED NULL | EM=0.00 wins |
| Donchian sleeve | REJECTED | 63% < 69.1% guardrail |
| Mid-caps | REJECTED | 60% < 70% threshold |
| 4h Multi-Timeframe Turtle | GRAVEYARD | 1/20 pass — structural timeframe incompatibility |
| BollingerReversion | GRAVEYARD | 0/288 OOS — signal actively harmful |
| BTC-ETH cointegration | GRAVEYARD | All 12 configs negative Sharpe, -16 to -46% return |
| ATR-norm position sizing | REJECTED | Equal capital optimal; ATR-norm inverts vol ranking |
| A/D Dual-Hat static sleeve | REJECTED | Below-random win rate |
| CTREND fixed 25% sleeve | REJECTED | Sharpe destroyed 1.38→0.33 |

---

## Anti-Overfitting Rules (Updated 2026-05-04)

1. Minimum 3-window (5.5%) improvement on OOS before accepting any param change
2. No sequential optimization on same data (EP=24 lesson — applies to any 3rd param on same harness)
3. Never re-run confirmed params at higher resolution on the same harness (VL=96 lesson — RESOLVED)
4. Held-out validation required for marginal wins (< 3 windows over baseline)
5. Equity curve dominance required (>80% of time bars)
6. **Absolute guardrails over relative improvement** (Donchian: +11% Sharpe but 63% pass < 69.1% guardrail → REJECTED)
7. **Sequential optimization detection:** When 3+ params optimized on same harness in sequence, latest param needs held-out validation before trust
8. **Equity numbers must match the actual live path** — do not cite dual-exit equity as live-equity when live bot is Turtle-only

---

## Key Insight: All Metrics Are Upper Bounds

Everything in HALL_OF_FAME.md is a simulation maximum. The real validation path is live testnet paper trading + comparing actual vs predicted metrics.

**Biggest unmeasured risk:** ATR_RANK=5 is production but has no equity run. We don't know the real equity of what we're running.

**Fee model reality check:** Backtest uses 10bps taker, but microstructure analysis shows ~70% maker fills on entries → real expected cost ~4-5bps. Backtest may be pessimistic by 2-3x. Live could outperform walk-forward numbers.

**Source of truth for production params: `src/live/config.rs`. Last verified: 2026-05-04.**
