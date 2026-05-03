# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-03 08:05 UTC. Critique cycle. ATR_RANK=24 extensive sweep done (T∈[0..100]). VL=96 validated in live_compatible_wf.rs (54/63 pass, Sharpe 7.652). Config drift: live_compatible_wf.rs has VL=96, config.rs has VL=8 — UNRECONCILED. Three ideas overdue 3-4 weeks: LOB NOBI, BTC-ETH cointegration, ETF flow. Live testnet BLOCKED 5+ weeks.**

---

## Critique Findings (2026-05-03)

**Core judgment:** The research loop is more productive than last cycle — ATR_RANK extensive sweep (T∈[0..100] × 9 universes × 7 windows) and VL=96 validation are real hyperopt results. But three genuinely novel ideas (LOB NOBI, BTC-ETH cointegration, ETF flow) were proposed 3-4 weeks ago and remain unbuilt. We generate at 10x the rate we test.

**ATR_RANK=24 same-harness artifact risk (UNRESOLVED):** Found on `live_compatible_wf.rs`, validated extensively on the same harness (T∈[0..100]). EP=24 failed held-out after being found on the same harness. ATR_RANK=24 has NOT been held-out validated. The 54/63 pass rate reflects in-sample OOS optimization on a specific grid. Must run held-out validation before citing as truth.

**Config drift (NEW FINDING):** `examples/live_compatible_wf.rs:32` has `VOL_LOOKBACK=96` while `src/live/config.rs:47` has `VOL_LOOKBACK=8`. The walk-forward harness validates VL=96 but the deployed bot uses VL=8. These are 12x different smoothing windows — the harness tests a different strategy than what's deployed. **Must reconcile before next commit.**

**Genuinely promising unbuilt (overdue 3-4 sessions each):**
- LOB NOBI signal: daemon running since 1862b57, data collected, signal NEVER tested — lowest lift, highest value
- BTC-ETH cointegration: first credible mean-reversion candidate with academic backing — proposed 4 weeks ago, zero commits
- ETF flow: institutional signal from public data (CoinGlass/Farside), never built

**Bear market gap:** 2018-2019 (BTC -83%, 12+ months) NOT in any walk-forward window. A grinding prolonged bear has never been tested.

**Fee model inconsistent:** A/D (3933 trades) may have different fee treatment than Turtle (397 trades). A/D Sharpe 3.61 vs Turtle Sharpe 0.98 — the 10x trade count gap needs audit.

**Three unbuilt ideas overdue:**
1. LOB NOBI signal test — data collected, one harness run, 4 sessions overdue
2. BTC-ETH cointegration — proposed 4 weeks ago, zero commits
3. ETF flow institutional signal — proposed 3 weeks ago, zero commits

---

## Current Truth

- `src/live/bot.rs` live path is **Turtle-only exit**; no Chandelier exit exists in the deployed bot.
- ATR_RANK=24: 54/63 pass (85.7%), Sharpe 7.652, Base5 aggregate 251.8x — candidate, NOT production default until held-out validated.
- **VOL_LOOKBACK CONFLICT:** live_compatible_wf.rs uses VL=96, config.rs uses VL=8. Must reconcile before trust.
- `progress_equity_curves.rs` Turtle+Chandelier: 110.1x / Sharpe 0.98 (daily equity, honest baseline).

---

## Production Params (Frozen — NEEDS RECONCILIATION)

```text
EP                  = 21
TURTLE_ATR_P        = 24
TURTLE_ATR_M        = 2.0
HOLD_MAX            = 12
POSITION_CAP        = 3
FRESHNESS_COOLDOWN  = 0
ATR_ENTRY_MULT      = 0.00
REGIME_ATR_P        = 12      ✅ confirmed optimal
REGIME_LOOKBACK     = 42      ✅ confirmed optimal (196-value LB sweep)
ATR_RANK_THRESHOLD  = 24      ⚠️ candidate — needs held-out validation
VOL_LOOKBACK        = 8       ⚠️ CONFLICT: live_compatible_wf uses 96
```

---

## Next Tasks (Priority Order)

### T49: LOB NOBI Signal Harness (IMMEDIATE — 1 session)
**Status:** READY — data already collected (daemon running since 1862b57), one harness file.
- Read LOB daemon data from `data/cache/lob_nobi/`
- Compute: daily depth imbalance `(bidQty-askQty)/(bidQty+askQty)` per symbol → SG smoothing → z-score
- Test: NOBI z-score > threshold predicts next-24h directional continuation vs zero baseline
- If edge exists → build microstructure sleeve. If null → GRAVEYARD cleanly.
- **Why:** Lowest-lift highest-value test in project history. Data already there. One new file.

### T50: Reconcile VOL_LOOKBACK (URGENT — config integrity)
**Status:** UNRECONCILED — live_compatible_wf.rs has VL=96, config.rs has VL=8.
- Run both VL=8 and VL=96 on Base5 × 7 windows in live_compatible_wf.rs
- If VL=96 wins → update config.rs. If VL=8 wins → update live_compatible_wf.rs
- Commit reconciled value to BOTH files. Do not let this drift again.
- **Why:** A strategy where the harness and deployed bot use different params is not a valid result.

### T51: ATR_RANK=24 Held-Out Validation (HIGH — 1 session)
**Status:** UNBUILT — same-harness artifact risk (EP=24 pattern).
- Run live_compatible_wf.rs on pre-2021 data only with T=24 vs T=5
- If T=24 wins held-out → promote to production default in config.rs
- If T=24 loses → revert to T=5 or run new search
- **Why:** We learned this lesson with EP=24 (failed held-out after being found on same harness). ATR_RANK=24 is cited as production-ready but has never been held-out validated.

### T52: BTC-ETH Cointegration Harness (HIGH — 1-2 sessions)
**Status:** PROPOSED 4 weeks ago, zero commits.
- Every prior mean-reversion: GRAVEYARD (RSI, BollingerReversion, OFI, VPIN, 4h MR, 1h MR)
- BTC-ETH has academic backing (Frontiers Jan 2026, coefficient ~0.0587) — genuinely different mechanism
- Build: rolling Johansen test + spread z-score entry/exit
- Test: 9 universes × 6 walk-forward windows
- If positive → first credible mean-reversion strategy. If null → GRAVEYARD (clean kill).
- **Why:** Completely orthogonal to the entire directional trend book. First novel strategy class in months.

### T44: Config Cleanup — Remove Dead CHAND_PERIOD
**Status:** READY — confirmed dead code (0 effort).
- `examples/chand_p_live_sweep.rs` confirmed: ALL 56 CHAND_PERIOD values produce IDENTICAL Turtle-only results.
- Live Turtle path NEVER reads CHAND_PERIOD.
- Keep in config.rs but mark as "dual-exit research path only".
- **Why:** Clutter reduction. Zero risk change.

### T45: Mock Exchange (Bypass Live Testnet Blocker)
**Status:** UNBUILT. 5+ weeks blocked on API keys.
- Lightweight Rust HTTP mock for binance-rs-async endpoints
- Seed with historical 1m klines to simulate fills and slippage
- Would have caught the 2026-05-01 live exit bug before testnet
- Milestone: serve `/api/v3/klines` from historical data, return mock fills for market orders
- **Why:** Unblocks execution logic testing without credentials.

### T9: Live Testnet
**Status:** BLOCKED on Noah's Binance testnet API keys (5+ weeks). T45 bypasses this.

---

## Anti-Spin Rules

1. **VOL_LOOKBACK must be the same in live_compatible_wf.rs and config.rs. No drift.**
2. **Do not cite ATR_RANK=24 as "production default" until held-out validation completes.**
3. No more hyperopts on settled parameters (ATR_EMA, ATR_ENTRY_MULT, FRESHNESS_COOLDOWN, HOLD_MAX, CHAND_MULT confirmed 2-3× each).
4. If blocked on credentials, say so plainly.
5. **Max 2 sequential optimizations per harness before mandatory held-out validation.**
6. DDBudget Sharpe is milestone-aggregated — never compare directly to daily-equity Sharpe numbers.
7. **ATR_RANK=24 is a candidate pending held-out validation — NOT a production default.**
8. When 3+ params optimized on same harness in sequence, latest needs held-out validation.

---

## Anti-Overfit Rules

1. Minimum 3-window (5.5%) improvement on OOS before accepting any param change.
2. No sequential optimization on same data (EP=24 lesson).
3. Never re-run confirmed params at higher resolution on the same harness (VL=96 lesson).
4. Held-out validation required for marginal wins (< 3 windows over baseline).
5. Equity curve dominance required (>80% of time bars).
6. **Max 2 sequential optimizations per harness** before mandatory held-out validation.
7. **Same-harness artifact check:** If 3+ params optimized on same harness in sequence, latest param needs held-out before trust.
8. **Before promoting any strategy to production default, verify it beats current default on pass rate AND return.**

---

## Graveyard / Rejections

| Strategy | Result | Key Reason |
|---|---|---|
| T40 Regime-Adaptive Exit | REJECTED | baseline M=2.30 wins all configs |
| S6 close_losers I=5 | GRAVEYARD | Incompatible with Turtle-only live exit (0 trades). Dual-exit only. |
| Donchian sleeve | REJECTED | 34/54 pass (63%) < guardrail 69.1% |
| ATR_ENTRY_MULT=0.94 | REJECTED | Held-out 10/18 vs baseline 11/18 |
| ATR_ENTRY_MULT=0.85 | REJECTED | Held-out 10/18 vs baseline 11/18 |
| Mid-caps | REJECTED | 60% pass < 70% threshold |
| ATR-norm position sizing | REJECTED | Equal capital optimal; ATR-norm inverts vol ranking |
| Asymmetric exit | REJECTED | All configs identical to baseline |
| Position scaling overlays | GRAVEYARD | All failed — Chandelier already manages it |
| 4h Multi-Timeframe Turtle | GRAVEYARD | 1/20 pass — structural timeframe incompatibility |
| BollingerReversion | GRAVEYARD | 0/288 OOS — signal actively harmful |
| REGIME_ATR_PERIOD=64 | REJECTED | Same-harness artifact (3rd sequential opt on live_compatible_wf.rs) |
| VPIN crash-state overlay | GRAVEYARD | Signal-to-noise too low |
| OFI proxy | GRAVEYARD | Wrong normalization; abandoned |
| 1h/4h Mean Reversion | GRAVEYARD | All symbols negative Sharpe on full history |
| ATR-norm position sizing | REJECTED | Inverts dollar-volume ranking |
| VOL_LOOKBACK=96 | ⚠️ CONFLICT | live_compatible_wf uses 96; config.rs uses 8 — UNRECONCILED |

---

## Unbuilt High-Priority Ideas (Pipeline)

| Idea | Priority | Status |
|---|---|---|
| **T49: LOB NOBI signal** | HIGH | Data collected, one harness file away |
| **T52: BTC-ETH cointegration** | HIGH | First credible mean-reversion, proposed 4 weeks ago |
| **ETF flow institutional signal** | HIGH | Data publicly available, never built |
| Short-side sleeve | HIGH | 100% long book gap, proposed 2026-04-05 |
| DXY-Realized-Vol regime gate | MEDIUM | BTC now liquidity-sensitive risk asset |
| LOB NOBI daily aggregate (arxiv 2602.00776) | HIGH | Market-cap normalized, Binance public endpoint |
| Stablecoin exchange reserve state | MEDIUM | Binance public API, no auth required |
| Leverage-fragility state (Oct 2025 mechanics) | MEDIUM | Funding data in cache, proxy bug needs fix |

---

## Project Status (2026-05-03)

**Research loop:** PRODUCTIVE this cycle (ATR_RANK extensive sweep, VL=96 validation are real). But novel ideas still unbuilt after 3-4 weeks.

**What needs to happen this week:**
1. T49 — LOB NOBI harness (one file, data already there)
2. T50 — VL reconciliation (config integrity, 30 minutes)
3. T51 — ATR_RANK held-out validation (1 session)
4. T52 — BTC-ETH cointegration (1-2 sessions)

**Remaining blocker:** Live testnet (Noah's API keys, 5+ weeks). T45 mock exchange bypasses this.