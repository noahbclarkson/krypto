# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-04 00:18 UTC. Critique cycle complete.**
- ATR_RANK=24 GRAVEYARD'd (held-out: 10/22 pass, Sharpe -0.964 vs T=5 14/22/+0.664) ✅
- VL=96 reconciled in config.rs ✓
- Short-side sleeve GRAVEYARD'd (37.5% pass vs 69.1% guardrail) ✓
- SIZE_MULT overlay INERT (cosmetic risk knob, M=0.70 confirmed) ✓
- **daily_progress.csv still has stale ATR_RANK=24 entry** (GRAVEYARD'd but reported as live)
- **ATR_RANK=5 equity run NEVER EXECUTED** — live config has no equity metric
- **data/cache/lob_nobi/ is EMPTY** — T55 is multi-session, not "one run away"
- Live testnet BLOCKED 5+ weeks on Noah's API keys — T53 mock exchange workaround identified, zero commits

---

## Critique Findings (2026-05-04)

**Core judgment:** 3/8 recent commits produce results. Project has closed the easy items (T52 rejection, T51 GRAVEYARD, VL reconciliation). Remaining items are genuinely multi-session (LOB NOBI data collection, mock exchange infrastructure). ATR_RANK=5 has no equity run despite being the production live config.

**Critical finding — daily_progress.csv contains GRAVEYARD'd strategy:**
- `2026-05-03,Turtle+ATR_RANK=24,...` — row from AFTER T52 rejected ATR_RANK=24 via held-out validation
- ATR_RANK=5 has NO entry in daily_progress.csv — never run
- The live bot is running a regime filter we have no equity metric for

**Critical finding — HALL_OF_FAME equity is wrong harness:**
- HOF says "113.6x / Sharpe 0.99" from `progress_equity_curves.rs` — dual Chandelier exit, NOT live Turtle-only path
- Live bot: Turtle-only with ATR_RANK=5 filter
- We have never run `progress_equity_curves.rs` with Turtle-only + ATR_RANK=5
- 113.6x is NOT the live bot's equity

**Critical finding — LOB NOBI data is missing:**
- `data/cache/lob_nobi/` is empty (0 files)
- `examples/depth_imbalance_pipeline.rs` is a 6-line stub with hardcoded print — never connected to real data
- T55 is back to zero: needs daemon → data persistence → harness → validation (multi-session)

**Critical finding — 5-week testnet blocker, no workaround executed:**
- T53 mock exchange (Rust HTTP server seeded with historical 1m data) identified as bypass
- Zero commits toward it
- Every research item would benefit from execution testing; we are research-paralyzed

**Bear market gap unchanged:** 2018-2019 (BTC -83%, 12+ months grinding) is NOT in any walk-forward window. Sharpe numbers inflated by bull-bias.

---

## Current Truth

- `src/live/bot.rs` live path is **Turtle-only exit**; no Chandelier exit in deployed bot.
- `ATR_RANK=5` is production regime filter (config.rs); **no equity run exists for this config**.
- `progress_equity_curves.rs` produces dual-exit equity (Chandelier+Turtle) — NOT the live bot.
- HOF "113.6x / Sharpe 0.99" is wrong harness — should be cited as dual-exit reference, not live equity.
- `reports/daily_progress.csv` needs ATR_RANK=5 entry and stale T=24 row removed.

---

## Next Tasks (Priority Order)

### T54: Run ATR_RANK=5 Equity (IMMEDIATE — 10 min)
**Status:** UNBUILT. ATR_RANK=5 is in production config; we have never measured its equity.
- Add ATR_RANK=5 variant to `progress_equity_curves.rs` (or create dedicated harness)
- Run full timeline: Turtle-only exit + ATR_RANK=5 regime filter + production params
- Update HALL_OF_FAME.md with correct live-equity figure
- Clean `reports/daily_progress.csv`: remove stale T=24 row, add T=5 entry
- **Why:** Flying production blind on the one strategy actually running live. 10 min to fix.

### T53: Mock Exchange Bypass (HIGH — 2-3 sessions)
**Status:** UNBUILT, identified 5+ weeks ago, zero commits.
- Live testnet BLOCKED on Noah's Binance testnet API keys for 5+ weeks
- Build local Rust HTTP server that mocks the binance-rs-async endpoints we use
- Seed with historical 1m data from `data/cache/` to simulate fills and slippage
- Test `src/live/bot.rs` order placement, state machine, latency handling
- **Why:** Highest-leverage unbuilt infrastructure. Execution testing unblocks all downstream validation.
- Data present: `data/cache/` has 1m parquet for BTC, ETH, SOL, XRP, DOGE

### T55: LOB NOBI Data Collection (MEDIUM — multi-session)
**Status:** DATA MISSING. `data/cache/lob_nobi/` is empty. `depth_imbalance_pipeline.rs` is a stub.
- Build collector daemon: fetch Binance depth API → aggregate → persist to parquet
- Run for at least 2 weeks to get sufficient market microstructure data
- Then build walk-forward harness: daily depth imbalance → SG smoothing → z-score → directional continuation
- **Why:** arxiv 2602.00776 — genuinely novel microstructure edge. Worth the multi-session investment.
- Note: NOT "one run away" — needs daemon + data persistence + harness + validation (multi-session)

---

## Production Params (Frozen — 2026-05-04)

```text
EP                  = 21     // Turtle entry lookback
TURTLE_ATR_P        = 24     // Turtle ATR stop period
TURTLE_ATR_M        = 2.0    // Turtle ATR stop multiplier
HOLD_MAX            = 12     // Max hold bars
POSITION_CAP        = 3      // Max concurrent positions
FRESHNESS_COOLDOWN  = 0
ATR_ENTRY_MULT      = 0.00
REGIME_ATR_P        = 12     // BTC ATR period for regime filter
REGIME_LOOKBACK     = 42     // BTC ATR percentile lookback
ATR_RANK_THRESHOLD  = 5.0    // Minimum BTC ATR percentile rank for entries
VOL_LOOKBACK        = 96     // Reconciled 2026-05-01
```

---

## Anti-Spin Rules

1. **No hyperopts on settled parameters without new mechanism.**
2. **Do not cite ATR_RANK=24 as anything — GRAVEYARD'd.**
3. **HOF equity is dual-exit reference. Live-equity for Turtle-only + ATR_RANK=5 is TBD (T54).**
4. **If blocked on credentials, say so plainly and build the workaround.**
5. **Max 2 sequential optimizations per harness before mandatory held-out validation.**
6. **All equity numbers are STALE until T54 runs.**

---

## Graveyard / Rejections

| Strategy | Result | Key Reason |
|---|---|---|
| ATR_RANK=24 | GRAVEYARD | Held-out: 10/22 pass, Sharpe -0.964 vs T=5 14/22/+0.664. Same-harness artifact. |
| Short-side sleeve | GRAVEYARD | 37.5% pass vs 69.1% guardrail. Signal too sparse (175 trades/253 windows). |
| AP=64 | REJECTED | Sequential optimization on same harness (EP=24 pattern). AP=12 wins held-out. |
| EP=24 | REVERTED | Held-out: 25/29 vs EP=21 27/29. Same-harness artifact. |
| REGIME_ATR_PERIOD=64 | REJECTED | Sequential optimization on same harness (EP=24 pattern). |
| VL=96 | ⚠️ RECONCILED | Was same-harness artifact risk. Reconciled 2026-05-01: both config.rs and live_compatible_wf.rs now use 96. |
| SIZE_MULT overlay | INERT | Return/DD scale linearly with M — pure risk knob, no alpha. M=0.70 confirmed. |
| BTC-ETH cointegration | GRAVEYARD | All 12 configs negative Sharpe, -16 to -46% return. |
| 4h Multi-Timeframe Turtle | GRAVEYARD | 1/20 pass — structural timeframe incompatibility |
| Donchian sleeve | REJECTED | 63% pass < 69.1% guardrail |
| BollingerReversion | GRAVEYARD | 0/288 OOS — signal actively harmful |
| ATR_ENTRY_MULT (all) | REJECTED | EM=0.00 wins definitively |
| ATR-norm position sizing | REJECTED | Inverts dollar-volume ranking |

---

## Unbuilt High-Priority Ideas (Pipeline)

| Idea | Priority | Status |
|---|---|
| **T54: ATR_RANK=5 equity run** | IMMEDIATE | Never run — production is flying blind |
| **T53: Mock exchange** | HIGH | 5+ weeks blocked, zero commits |
| **T55: LOB NOBI data collection** | MEDIUM | Data missing, multi-session |
| ETF flow institutional signal | MEDIUM | Data publicly available, never built |
| DXY-Realized-Vol regime gate | MEDIUM | BTC now liquidity-sensitive risk asset |
| Stablecoin exchange reserve state | MEDIUM | Binance public API, no auth required |

---

## Project Status (2026-05-04)

**Research loop: COMPLETE on settled items.** ATR_RANK=24, short-side sleeve, VL reconciliation, SIZE_MULT — all closed. The easy confirmation hyperopts are done.

**What's genuinely overdue:**
1. T54: ATR_RANK=5 equity run (10 min — should have been done before going live)
2. T53: Mock exchange (2-3 sessions — identified 5 weeks ago, never started)
3. T55: LOB NOBI data collection (multi-session — data gap confirmed)

**Remaining blocker:** Live testnet (Noah's API keys, 5+ weeks). T53 mock exchange bypasses this.
