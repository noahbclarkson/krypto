# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-04 00:18 UTC. Critique cycle complete. ATR_RANK=24 GRAVEYARD'd (held-out). VL=96 reconciled. Short-side sleeve GRAVEYARD'd. SIZE_MULT INERT (cosmetic risk knob). T54 ATR_RANK=5 equity run is top immediate priority. T53 mock exchange is highest overdue infrastructure. T55 LOB NOBI data MISSING — multi-session.*

---

## Critical Alert: ATR_RANK=24 GRAVEYARD'd

**ATR_RANK=24 REJECTED via held-out validation (a324a0fa, 2026-05-04).**
- Pre-2021 held-out: T=24 → **10/22 pass, Sharpe -0.964** vs T=5 → **14/22 pass, Sharpe +0.664**
- Same EP=24 pattern: 3rd sequential optimization on same harness → failed held-out
- Production: **ATR_RANK_THRESHOLD = 5.0** (reverted from 24)

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

### #1: T54 — Fresh ATR_RANK=5 Equity Run (IMMEDIATE — ⚠️ RE-RUN REQUIRED)
**Status:** STALE DATA. Equity was run (43.1x/0.87) but BEFORE live-bot Turtle-only ATR fix (2026-05-01). The 43.1x is from broken code.
- Re-run `progress_equity_curves.rs` Turtle-only + ATR_RANK=5 on current (fixed) code
- Update HOF and daily_progress.csv with correct figure
- **Why:** Every equity number for live strategy is wrong or missing.

### #2: T53 — Mock Exchange Bypass (HIGH — 2-3 sessions)
**Status:** UNBUILT, 5+ weeks overdue.
- Live testnet BLOCKED on Noah's API keys for 5+ weeks
- Build Rust HTTP server that mocks binance-rs-async endpoints we use
- Seed with historical 1m data from `data/cache/` to simulate fills and slippage
- Test `src/live/bot.rs` order placement, state machine, latency handling
- **Why:** Highest-leverage unbuilt item. Execution testing unblocks all downstream validation. Data exists.

### #3: T55 — LOB NOBI Data Collection (MEDIUM — multi-session)
**Status:** DATA MISSING. `data/cache/lob_nobi/` is empty. Multi-session project — NOT "one run away".
- Build collector daemon: Binance depth API → parquet persistence (run 2+ weeks)
- Then build harness: daily depth imbalance → SG smoothing → z-score → directional continuation test
- arxiv 2602.00776 — genuinely novel microstructure edge.
- **Why:** Worth the multi-session investment. Stop claiming it's close.

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
