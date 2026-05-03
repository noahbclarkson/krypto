# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-03 16:05 UTC. Critique cycle complete.** VL=96 vs VL=8 config drift UNRESOLVED. ATR_RANK=24 same-harness artifact risk UNVALIDATED. All Turtle-only equity numbers STALE (progress_equity_curves.rs not re-run after 2026-05-01 live exit bug fix). Three tasks overdue: LOB NOBI (3+ weeks), short-side sleeve (4 weeks), VL reconciliation (urgent). Live testnet BLOCKED 5+ weeks.

---

## Critique Findings (2026-05-03)

**Core judgment:** 8 recent commits: 3 valid results (37%), 5 meta-work/docs. The project is generating at 10x the rate it tests. BTC-ETH GRAVEYARD (6f0af262) was the only result-producing commit in 2 sessions. The rest is documentation cycling.

**Critical finding — Config drift (URGENT):** `src/live/config.rs:47` has `VOL_LOOKBACK=8`; `examples/live_compatible_wf.rs:32` has `VOL_LOOKBACK=96`. The walk-forward harness validates VL=96 (54/63 pass, 251.8x) but the deployed bot uses VL=8. These are 12x different smoothing windows. **The HALL_OF_FAME headline "251.8x / 85.7% pass" is for VL=96. The live bot runs VL=8.** Must reconcile before citing any VL=96 result as production-trusted.

**Critical finding — ATR_RANK=24 same-harness artifact risk (UNRESOLVED):** Found as 3rd sequential optimization on `live_compatible_wf.rs` (after REGIME_LOOKBACK=42 and REGIME_ATR_PERIOD=12). EP=24 failed held-out after being found on the same harness. ATR_RANK=24 has NOT been held-out validated. The 54/63 pass rate reflects in-sample OOS optimization on a specific grid. **ATR_RANK=24 is a candidate, NOT a production default.**

**Critical finding — All Turtle-only equity numbers are STALE:** The 2026-05-01 live exit bug fix corrected `src/live/bot.rs`. `live_compatible_wf.rs` was re-run under corrected semantics. But `progress_equity_curves.rs` has NOT been re-run. All Turtle-only equity figures (Turtle ATR_RANK=24: 77.4x / 0.97 Sharpe) are from pre-fix code. Must re-run.

**Bear market gap:** 2018-2019 (BTC -83%, 12+ months of grinding decline) is NOT in any walk-forward window. The worst stress tests are sharp V-shape crashes (W02 COVID). A grinding 12-month bear has never been tested. Sharpe numbers are inflated by bull-bias in OOS data.

**Three overdue unbuilt ideas:**
1. LOB NOBI signal — data collected, one harness file away, 3+ weeks overdue
2. Short-side sleeve — proposed 2026-04-05, zero commits, 4 weeks overdue
3. VOL_LOOKBACK reconciliation — config drift, 12x difference, urgent

---

## Current Truth

- `src/live/bot.rs` live path is **Turtle-only exit**; no Chandelier exit exists in the deployed bot.
- `live_compatible_wf.rs` (corrected 2026-05-01): ATR_RANK=24, VL=96 — 54/63 pass (85.7%), Sharpe 7.652, Base5 251.8x — candidate metrics, NOT production-trusted until reconciliation.
- **VOL_LOOKBACK CONFLICT:** config.rs has VL=8, live_compatible_wf.rs has VL=96. Harness and deployed bot test different strategies.
- `progress_equity_curves.rs`: STALE — not re-run after 2026-05-01 live exit bug fix.

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
ATR_RANK_THRESHOLD  = 24.0    ⚠️ candidate — needs held-out validation (same-harness artifact risk)
VOL_LOOKBACK        = 8       ⚠️ CONFLICT: live_compatible_wf uses 96 — UNRECONCILED
```

---

## Next Tasks (Priority Order)

### T49: LOB NOBI Signal Harness (IMMEDIATE — 1 session)
**Status:** 3+ WEEKS OVERDUE — data collected in `data/cache/lob_nobi/`, one harness file away.
- `examples/depth_imbalance_pipeline.rs` is a 6-line stub — replace with proper walk-forward harness
- Compute: daily depth imbalance `(bidQty-askQty)/(bidQty+askQty)` → SG smoothing → z-score
- Test: NOBI z-score > threshold predicts next-24h directional continuation vs zero baseline
- If edge exists → build microstructure sleeve. If null → GRAVEYARD cleanly.
- **Why:** Lowest lift, highest value in project history. Data already there. One new file.

### T50: Reconcile VOL_LOOKBACK + Re-run progress_equity_curves.rs (URGENT — 30 min)
**Status:** UNRECONCILED — config.rs VL=8, live_compatible_wf.rs VL=96. Also: all Turtle equity numbers stale.
- Run both VL=8 and VL=96 on Base5 × 7 windows in live_compatible_wf.rs
- If VL=96 wins → update config.rs. If VL=8 wins → update live_compatible_wf.rs
- Commit reconciled value to BOTH files. No drift.
- **Also:** Re-run `cargo run --example progress_equity_curves --profile sweep` with corrected live exit semantics
- All "Turtle+Chandelier" and "Turtle ATR_RANK=24" equity numbers in HALL_OF_FAME are stale until re-run
- **Why:** A strategy where the harness and deployed bot use different params is not a valid result. Stale equity undermines all reporting.

### T51: Short-Side Sleeve (HIGH — 1-2 sessions)
**Status:** PROPOSED 2026-04-05, zero commits. 4+ weeks overdue.
- Book is 100% long — structural liability in bear regimes (2026 YTD: Turtle -22.7% vs BTC -14.2%)
- Simple hypothesis: BTC 21d vol > 90th pct of 252d AND SMA21 < SMA200 → take 5% short position
- Test: Base5 × 7 windows. If pass ≥ 69% → HOF candidate. If fail → GRAVEYARD cleanly.
- **Why:** Crisis alpha is uncorrelated with trend-following. Completely different mechanism from any prior strategy.
- Mechanism is NOT another entry filter variant — it's a genuinely new strategy class

### T52: ATR_RANK=24 Held-Out Validation (MEDIUM — 1 session, after T50)
**Status:** PENDING — same-harness artifact risk (EP=24 pattern).
- Run live_compatible_wf.rs on pre-2021 data only with T=24 vs T=5
- If T=24 wins held-out → promote to production default in config.rs
- If T=24 loses → revert to T=5.0 or run fresh search.
- **Why:** ATR_RANK=24 found as 3rd sequential optimization on same harness. Same pattern as EP=24 which failed held-out.

### T44: Config Cleanup — Remove Dead CHAND Code
**Status:** READY — confirmed dead code (0 effort).
- `src/live/bot.rs` is Turtle-only exit — Chandelier is NEVER invoked
- `src/live/config.rs` still defines `CHAND_PERIOD` and `CHAND_MULT` — creates false impression Chandelier is active
- **Action:** Add `#[allow(dead_code)]` with comment `// INERT: live bot uses Turtle-only exit; Chandelier confirmed dead on live path`
- **Why:** Config clarity. Every reader of config.rs sees CHAND_PERIOD=7 and assumes it does something.

### T9: Live Testnet
**Status:** BLOCKED on Noah's Binance testnet API keys (5+ weeks). T45 mock exchange bypasses this.

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
9. **All equity numbers are STALE until progress_equity_curves.rs is re-run with corrected live exit semantics.**

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
| BTC-ETH cointegration | GRAVEYARD | All 12 configs negative Sharpe, -16 to -46% return (2026-05-01) |
| VPIN crash-state overlay | GRAVEYARD | Signal-to-noise too low |
| OFI proxy | GRAVEYARD | Wrong normalization; abandoned |
| 1h/4h Mean Reversion | GRAVEYARD | All symbols negative Sharpe on full history |
| ATR-norm position sizing | REJECTED | Inverts dollar-volume ranking |
| VOL_LOOKBACK=96 | ⚠️ CONFLICT | live_compatible_wf uses 96; config.rs uses 8 — UNRECONCILED |
| ATR_RANK=24 | ⚠️ CANDIDATE | 3rd sequential optimization on live_compatible_wf.rs — needs held-out |

---

## Unbuilt High-Priority Ideas (Pipeline)

| Idea | Priority | Status |
|---|---|
| **T49: LOB NOBI signal** | HIGH | Data collected, one harness file away — 3+ weeks overdue |
| **T51: Short-side sleeve** | HIGH | Proposed 2026-04-05, zero commits — 4+ weeks overdue |
| **T50: VOL_LOOKBACK reconciliation** | URGENT | Config drift: config.rs VL=8, live_compatible_wf.rs VL=96 |
| **Re-run progress_equity_curves.rs** | URGENT | All Turtle-only equity numbers stale after 2026-05-01 bug fix |
| **T52: ATR_RANK=24 held-out** | MEDIUM | Same-harness artifact risk (EP=24 pattern) — after T50 |
| ETF flow institutional signal | MEDIUM | Data publicly available, never built |
| DXY-Realized-Vol regime gate | MEDIUM | BTC now liquidity-sensitive risk asset |
| LOB NOBI daily aggregate (arxiv 2602.00776) | MEDIUM | Market-cap normalized, Binance public endpoint |
| Stablecoin exchange reserve state | MEDIUM | Binance public API, no auth required |
| Leverage-fragility state (Oct 2025 mechanics) | MEDIUM | Funding data in cache, proxy bug needs fix |

---

## Project Status (2026-05-03)

**Research loop:** MIXED — ATR_RANK extensive sweep and VL=96 validation are real hyperopt results. But 5/8 recent commits are meta-work/docs. The project is generating at 10x the rate it tests. BTC-ETH GRAVEYARD was the only result-producing commit in 2 sessions.

**What's genuinely unresolved:**
1. VOL_LOOKBACK config drift — harness and deployed bot use different values
2. ATR_RANK=24 same-harness artifact risk — never held-out validated
3. All equity numbers stale — progress_equity_curves.rs not re-run after live exit bug fix
4. LOB NOBI — data collected 3+ weeks ago, never tested
5. Short-side sleeve — proposed 4 weeks ago, zero commits

**What needs to happen this week:**
1. T49 — LOB NOBI harness (one file, data already there)
2. T50 — VL reconciliation + re-run progress_equity_curves.rs (30 min, urgent)
3. T51 — short-side sleeve (1-2 sessions)
4. T52 — ATR_RANK=24 held-out (after T50)

**Remaining blocker:** Live testnet (Noah's API keys, 5+ weeks). T45 mock exchange bypasses this.