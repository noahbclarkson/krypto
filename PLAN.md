# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-03 20:05 UTC. Critique cycle complete.**
- VL=96 reconciled ✅ (config.rs and live_compatible_wf.rs both now use 96)
- ATR_RANK=24 same-harness artifact risk UNRESOLVED (3rd sequential optimization, held-out pending)
- LOB NOBI data MISSING (data/cache/lob_nobi/ is empty — not "one harness run away")
- Short-side sleeve: 4 weeks overdue, zero commits
- Live testnet BLOCKED on Noah's Binance testnet API keys (6+ weeks)

---

## Critique Findings (2026-05-03 20:05 UTC)

**Core judgment:** 8 recent commits: 2/8 produce results (25%), 6/8 are docs/chore/meta. Documentation spiral confirmed, 4+ sessions running.

**Critical finding — LOB NOBI data is MISSING:** `data/cache/lob_nobi/` is empty (0 files). The "one harness run away" claim from prior sessions was WRONG. The LOB collector daemon ran but data was not persisted or the cache was cleared. T49 is a 2-3 session project (daemon → data persistence → harness), not 1 session.

**Critical finding — ATR_RANK=24 same-harness artifact risk UNRESOLVED:** Three sequential optimizations on `live_compatible_wf.rs`: REGIME_LOOKBACK=42 (#2), REGIME_ATR_PERIOD=12 (#2), ATR_RANK=24 (#3). EP=24 (3rd optimization on its harness) FAILED held-out. AP=64 (3rd optimization) FAILED held-out. ATR_RANK=24 has the same structural pattern. Pending held-out validation on pre-2021 data comparing T=24 vs T=5 vs T=0.

**Bear market gap:** The worst stress tests are sharp V-shape crashes (W02 COVID). A grinding 12-month bear (2018-2019: BTC -83%, 12+ months) is NOT in any walk-forward window. Sharpe numbers inflated by bull-bias in OOS data.

**Misleading metrics in reports/daily_progress.csv:** The dual-exit "Turtle+Chandelier" (111.4x / 0.98 Sharpe) is NOT the live bot. The live bot is Turtle-only. `daily_progress.csv` should drop or relabel the dual-exit row. DDBudget 7.20 Sharpe (milestone-aggregated) is not comparable to Turtle 0.98 (daily equity) — apples-to-oranges inflation.

---

## Current Production Params (Frozen — ATR_RANK=24 Pending Held-Out)

```text
EP                  = 21
TURTLE_ATR_P        = 24
TURTLE_ATR_M        = 2.0
HOLD_MAX            = 12
POSITION_CAP        = 3
FRESHNESS_COOLDOWN  = 0
ATR_ENTRY_MULT      = 0.00
CHAND_PERIOD        = 7       // INERT in live path (Turtle-only exit)
CHAND_MULT         = 2.30    // INERT in live path
REGIME_ATR_P        = 12      ✅ confirmed optimal
REGIME_LOOKBACK     = 42      ✅ confirmed optimal
ATR_RANK_THRESHOLD  = 24.0    ⚠️ CANDIDATE — 3rd sequential opt, held-out pending
VOL_LOOKBACK        = 96      ✅ reconciled (was VL=8 in config.rs)
```

---

## Next Tasks (Priority Order)

### T49: LOB NOBI Signal Harness — REDOWN + HARNESS (MULTI-SESSION)
**Status:** DATA MISSING — `data/cache/lob_nobi/` is empty.
- Phase 1: Re-run LOB collector daemon, persist data to `data/cache/lob_nobi/`
- Phase 2: Build proper walk-forward harness (`examples/lob_nobi_walkforward.rs`)
- Compute: daily depth imbalance `(bidQty-askQty)/(bidQty+askQty)` → SG smoothing → z-score
- Test: NOBI z-score > threshold predicts next-24h directional continuation vs zero baseline
- If edge exists → build microstructure sleeve. If null → GRAVEYARD cleanly.
- **Why:** Lowest lift, highest value if data is there. Re-assessed as 2-3 sessions, not 1.

### T51: Short-Side Sleeve (HIGH — 1 session, 4 WEEKS OVERDUE)
**Status:** PROPOSED 2026-04-05, zero commits.
- Book is 100% long — structural liability in bear regimes (2026 YTD: Turtle -22.7% vs BTC -14.2%)
- Hypothesis: BTC 21d vol > 90th pct of 252d AND SMA21 < SMA200 → take 5% short position
- Test: Base5 × 7 windows. If pass ≥ 69% → HOF candidate. If fail → GRAVEYARD cleanly.
- **Why:** Crisis alpha is uncorrelated with trend-following. Completely different mechanism.
- `examples/crisis_short_sleeve_walkforward.rs` exists but is a SHORT overlay, not short-side allocation

### T52: ATR_RANK=24 Held-Out Validation (URGENT — 1 session)
**Status:** PENDING — same-harness artifact risk (EP=24 pattern).
- Run `live_compatible_wf.rs` on pre-2021 data only comparing T=24 vs T=5 vs T=0
- If T=24 wins held-out across all pre-2021 regimes → confirm as production default
- If T=24 loses → revert to T=5.0 (conservative prior default)
- **Why:** ATR_RANK=24 is 3rd sequential optimization on same harness. EP=24 and AP=64 both failed held-out with same pattern. Must validate before trusting.

### T53: Mock Exchange Decision (1 session — triage)
**Status:** UNRESOLVED — `src/live/mock_exchange.rs` (703 lines) never integrated.
- Option A: Integrate into `live_turtle_chandelier.rs` test harness (connects dead code)
- Option B: Delete (703 lines of dead code adding compile time and cognitive load)
- Decision criterion: If integration takes >1 session, delete it.
- **Why:** Dead code is a maintenance burden. Decide and act.

### T54: daily_progress.csv Label Cleanup (LOW — 30 min)
**Status:** READY — identified this session.
- Drop or relabel dual-exit "Turtle+Chandelier" row (111.4x / 0.98 Sharpe — NOT the live bot)
- Keep only Turtle ATR_RANK=24 live variant
- Add methodology labels to prevent Sharpe comparison errors
- **Why:** Reports should not compare milestone-aggregated DDBudget to daily equity Turtle

### T44: Config Cleanup — Remove Dead CHAND Code (READY)
**Status:** READY — confirmed dead code (0 effort).
- Add `#[allow(dead_code)]` with comment `// INERT: live bot uses Turtle-only exit; Chandelier confirmed dead on live path`
- CHAND_PERIOD=7 and CHAND_MULT=2.30 in config.rs create false impression Chandelier is active

### T9: Live Testnet (BLOCKED — API keys 6+ weeks)
**Status:** BLOCKED on Noah's Binance testnet API keys (6+ weeks). T45 mock exchange bypasses this.

---

## Anti-Spin Rules (Updated 2026-05-03)

1. **LOB NOBI is NOT "one harness run away." Data is missing. T49 is a multi-session project.**
2. **Do not cite ATR_RANK=24 as "production default" until held-out validation completes.**
3. **No more hyperopts on settled parameters** (ATR_EMA, ATR_ENTRY_MULT, FRESHNESS_COOLDOWN, HOLD_MAX, CHAND_MULT confirmed 2-3× each).
4. **Max 2 sequential optimizations per harness before mandatory held-out validation.**
5. **Same-harness artifact detection:** 3rd sequential optimization = held-out required.
6. **All equity numbers STALE until re-run with corrected live exit semantics** (progress_equity_curves.rs — TBD).
7. **Do not compare milestone-aggregated Sharpe to daily equity Sharpe.**
8. **Reject pass-rate winners that degrade Sharpe** (AP=16 rejected; AP=12 confirmed).

---

## Graveyard / Rejections (Updated 2026-05-03)

| Strategy | Result | Key Reason |
|---|---|---|
| BTC-ETH cointegration | GRAVEYARD (6f0af262) | All 12 configs negative Sharpe, -16 to -46% return |
| ATR_ENTRY_MULT=0.94 | REJECTED | Held-out 10/18 vs baseline 11/18 |
| ATR_ENTRY_MULT=0.85 | REJECTED | Held-out 10/18 vs baseline 11/18 |
| ATR_EMA [1..200] | CONFIRMED NULL | No improvement anywhere in range |
| FRESHNESS_COOLDOWN [0..70] | CONFIRMED NULL | cd=0 optimal |
| HOLD_MAX [1..100] | CONFIRMED NULL | HM=12 optimal, no benefit from longer |
| CHAND_MULT [1.5..5.0] | CONFIRMED NULL | M=2.30 optimal |
| ATR_ENTRY_MULT [0.00..2.00] step 0.01 | CONFIRMED NULL | EM=0.00 wins |
| S6 close_losers I=5 | GRAVEYARD | Incompatible with Turtle-only live exit (0 trades) |
| Donchian sleeve | REJECTED | 63% < 69.1% guardrail |
| ATR-norm position sizing | REJECTED | Equal capital optimal; ATR-norm inverts vol ranking |
| Asymmetric exit | REJECTED | All configs identical to baseline |
| Mid-caps | REJECTED | 60% < 70% threshold |
| 4h Multi-Timeframe Turtle | GRAVEYARD | 1/20 pass — structural timeframe incompatibility |
| BollingerReversion | GRAVEYARD | 0/288 OOS — signal actively harmful |
| Position scaling overlays | GRAVEYARD | All failed — Chandelier already manages it |
| Vol-contingent Chandelier (uniform) | GRAVEYARD | All configs identical — mechanism doesn't work |
| ATR entry × volume confirmation | REJECTED | 40 configs, all inferior to no filter |
| REGIME_ATR_PERIOD=64 | REJECTED | Sequential optimization (EP=24 pattern) |
| VPIN crash-state overlay | GRAVEYARD | Signal-to-noise too low |
| OFI proxy | GRAVEYARD | Wrong normalization; abandoned |
| 1h/4h Mean Reversion | GRAVEYARD | All symbols negative Sharpe on full history |
| BTC correlation entry filter | REJECTED | All variants lose to baseline on every metric |
| A/D Static Sleeve | REJECTED | Below-random win rate |
| CTREND Fixed 25% sleeve | REJECTED | Sharpe destroyed 1.38→0.33 |
| Donchian 25% sleeve | REJECTED | 63% < 69.1% production guardrail |
| Rebalancing trim_losers | REJECTED | Identical Sharpe, lower return |
| ATR_RANK_THRESHOLD=24 | ⚠️ CANDIDATE | 3rd sequential optimization on live_compatible_wf — held-out pending |
| LOB NOBI | ⚠️ DATA MISSING | data/cache/lob_nobi/ empty — multi-session project |

---

## Unbuilt High-Priority Ideas (Pipeline)

| Idea | Priority | Status |
|---|---|
| **T49: LOB NOBI — REDOWN + HARNESS** | HIGH | DATA MISSING — empty cache dir, multi-session |
| **T51: Short-side sleeve** | HIGH | Proposed 2026-04-05, zero commits — 4 weeks overdue |
| **T52: ATR_RANK=24 held-out** | URGENT | Same-harness artifact risk (EP=24 pattern) |
| **T53: Mock exchange decision** | MEDIUM | 703 lines dead code — integrate or delete |
| **T54: daily_progress.csv cleanup** | LOW | Drop dual-exit row, label methodology |
| ETF flow institutional signal | MEDIUM | Data publicly available, never built |
| DXY-Realized-Vol regime gate | MEDIUM | BTC now liquidity-sensitive risk asset |
| Stablecoin exchange reserve state | MEDIUM | Binance public API, no auth required |
| Leverage-fragility state (Oct 2025) | MEDIUM | Funding data in cache, proxy bug needs fix |

---

## Project Status (2026-05-03 20:05 UTC)

**Research loop CLOSED for directional strategies.** All trend-following params exhausted. All MR strategies GRAVEYARD. Only live testnet (blocked 6+ weeks) or genuinely new mechanisms remain.

**Genuinely unresolved (actionable this week):**
1. T52: ATR_RANK=24 held-out validation — 1 session, resolves artifact risk
2. T51: Short-side sleeve — 1 session, 4 weeks overdue
3. T53: Mock exchange decision — 1 session triage
4. T49: LOB collector re-run + data persistence — multi-session, data missing

**What needs to happen this week:**
1. T52 — ATR_RANK=24 held-out (1 session, URGENT)
2. T51 — short-side sleeve (1 session, HIGH)
3. T53 — mock exchange decision (1 session, triage)
4. Begin T49 Phase 1 — LOB collector re-run

**Remaining blocker:** Live testnet (Noah's API keys, 6+ weeks).
