# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-03 00:12 UTC. Critique cycle. Documentation spiral confirmed (4/8 recent commits). CHAND_PERIOD=7 is dead code in live Turtle path. ATR_RANK=24 has same-harness artifact signature as EP=24. LOB NOBI never tested (data collected 2026-04). BTC-ETH cointegration proposed 4 weeks ago, zero commits. Short-side sleeve: 100% long book, zero commits. Live testnet BLOCKED 5+ weeks.**

---

## Critique Findings (2026-05-03)

**Core judgment:** The research loop is a confirmation spiral, not a discovery loop. Last 8 commits: 4 pure docs, 2 confirming null results, 2 genuine kills. We keep auditing ourselves and finding nothing new. The "research loop closed" declaration has become a comfortable hammock.

**CHAND_PERIOD=7 is dead code (confirmed 2026-05-02):** `examples/chand_p_live_sweep.rs` (56 values, 3,528 runs) proved ALL values produce IDENTICAL results. The Turtle-only live path NEVER references CHAND_PERIOD. It was optimized in dual-exit research harness, integrated into config.rs, but never used in the live bot. This was 3 commits to confirm a mistake.

**ATR_RANK=24 has same-harness artifact signature as EP=24:** Found on `live_compatible_wf.rs` harness, then AP=64 and LB=42 were optimized sequentially on the same harness (sequential optimizations #2 and #3). EP=24 failed held-out validation. ATR_RANK=24 has NOT been held-out validated. The 52/63 pass rate reflects in-sample OOS optimization on a specific grid — it should be treated as a candidate, not a production default.

**Genuinely promising unbuilt (overdue 2+ sessions each):**
- LOB NOBI daemon running since 1862b57, signal NEVER tested — lowest lift, highest potential value
- BTC-ETH cointegration pair trade — proposed 2026-04-06, zero commits
- Short-side sleeve — 100% long book, proposed 2026-04-05, zero commits

**Bear market gap:** 2018-2019 (BTC -83% over 12+ months) NOT in any walk-forward window. 2022 was fast crash + fast recovery. A grinding prolonged bear has never been tested.

**Fee model unvalidated:** Backtest assumes 10bps taker. Microstructure analysis suggests ~70% maker fills → ~4-5bps real cost. We could be 2-3x more efficient than our simulations show, or we could be wrong.

**415 example files:** Most are graveyard clutter (~20-30 are relevant). Confuses codebase, slows onboarding, obscures production path.

**Three unbuilt ideas overdue:**
1. LOB NOBI signal test — data collected, one harness run
2. BTC-ETH cointegration — proposed 4 weeks ago
3. Short-side sleeve — 100% long book gap

---

## Progress: ATR_RANK=24 Awaiting Promotion

- Turtle equity (daily, honest): 110.1x / Sharpe 0.98
- ATR_RANK=24: **52/63 pass (82.5%), Sharpe 5.590, +132.3% avg return** ✅ Best validated strategy
- ATR_RANK=24 sits on plateau T=24-27 — robust to mis-specification ✅
- REGIME_LOOKBACK=42: confirmed optimal via 196-value sweep (LB=42-45 plateau) ✅
- REGIME_ATR_PERIOD=12: confirmed optimal ✅
- Live testnet: BLOCKED on Noah's API keys (5+ weeks)
- T40 (Regime-Adaptive Exit): REJECTED — baseline M=2.30 wins ✅
- T38 partial (live_compatible_wf.rs): credible but incomplete

---

## Current Truth

- `src/live/bot.rs` live path is **Turtle-only exit**; no Chandelier exit exists in the deployed bot.
- ATR_RANK=24: best validated strategy — 82.5% pass rate, Sharpe 5.590 — awaiting promotion to production default.
- VOL_LOOKBACK: defined in `src/live/config.rs` as 8 (conservative). VL=96 rejected as artifact.
- `progress_equity_curves.rs` CHAND_P=7 ✅
- Live bot equity (75.1x) from progress_equity_curves.rs — not from live_compatible_wf.rs export.

---

## Production Params (Frozen — VERIFIED 2026-05-02)

```text
EP                  = 21
TURTLE_ATR_P        = 24
TURTLE_ATR_M        = 2.0
HOLD_MAX            = 12
POSITION_CAP        = 3
FRESHNESS_COOLDOWN  = 0
ATR_ENTRY_MULT      = 0.00
REGIME_ATR_P        = 12      ✅ confirmed optimal
REGIME_LOOKBACK     = 42      ✅ confirmed optimal (196-value sweep)
ATR_RANK_THRESHOLD  = 24      ✅ confirmed (plateau T=24-27) — PROMOTION PENDING T42
VOL_LOOKBACK        = 8       ← conservative; VL=96 rejected as artifact (T37)
```

---

## Next Tasks (Priority Order)

### T42: ATR_RANK=24 Held-Out Validation (BEFORE PROMOTION)
**Status:** BLOCKED — same-harness artifact risk. NOT production default.
- ATR_RANK=24: 52/63 pass (82.5%), Sharpe 5.590, +132.3% avg OOS return
- SAME-HARNESS ARTIFACT RISK: Found on `live_compatible_wf.rs`, then AP=64 and LB=42 optimized sequentially on the same harness (sequential opts #2 and #3). Same pattern as EP=24 which failed held-out.
- Anti-spin rule: "When 3+ params are optimized on the same harness in sequence, the latest param needs held-out validation."
- Action: Run T42-held-out to validate on pre-2021 data. If wins → promote to config.rs. If loses → ATR_RANK=5 (well-validated old default) remains.
- Do NOT promote ATR_RANK=24 to config.rs without held-out validation.

### T43: LOB NOBI Signal Harness
**Status:** READY — data collected (daemon running since 1862b57), one harness run.
- Compute: daily top-5 depth imbalance `(bidQty-askQty)/(bidQty+askQty)` → SG smoothing → z-score signal
- Test: NOBI z-score > threshold predicts next-24h directional continuation vs zero baseline
- If positive edge → build microstructure sleeve. If null → GRAVEYARD cleanly.
- Lowest-lift highest-value test in entire program history. One new example file.

### T44: Config Cleanup — Remove Dead CHAND_PERIOD
**Status:** READY — confirmed dead code, 0 effort.
- `examples/chand_p_live_sweep.rs` confirmed: ALL 56 CHAND_PERIOD values produce IDENTICAL Turtle-only results.
- Live Turtle path NEVER reads CHAND_PERIOD.
- Remove `CHAND_PERIOD` and `CHAND_MULT` from `src/live/config.rs` (they are research artifacts in live execution).
- Exception: if we ever re-introduce dual-exit Chandelier path, these are needed. Keep in config but mark clearly as "dual-exit research path only".

### T45: Mock Exchange (Bypass Live Testnet Blocker)
**Status:** UNBUILT. 5+ weeks blocked on API keys.
- Lightweight Rust HTTP mock for binance-rs-async endpoints
- Seed with historical 1m klines to simulate fills and slippage
- Would have caught the 2026-05-01 live exit bug before testnet
- Unblocks execution logic testing without credentials — infinite iteration speed
- Milestone: serve `/api/v3/klines` from historical data, return mock fills for market orders

### T46: BTC-ETH Cointegration Pair Trade
**Status:** PROPOSED. First credible mean-reversion in pipeline.
- Every prior mean-reversion attempt is GRAVEYARD: RSI, BollingerReversion, OFI, VPIN, 4h MR, 1h MR
- BTC-ETH has academic backing (Frontiers Jan 2026, cointegrating coefficient ~0.0587)
- Build rolling Johansen test + spread z-score for entry/exit
- Completely orthogonal to entire existing directional trend book
- Medium complexity — one new harness file

### T47: Example File Hygiene Sprint
**Status:** UNBUILT. 415 files, ~20-30 relevant.
- Archive all graveyard/abandoned examples to `examples/archive/`
- Keep only: active production strategies, actively-validated candidates, core infrastructure (walk_forward.rs, progress_equity_curves.rs, etc.)
- Target: reduce from 415 → ~50 relevant files
- Makes codebase navigable, clarifies production path, speeds up CI

### T9: Live Testnet
**Status:** BLOCKED on Noah's Binance testnet API keys (5+ weeks). T45 bypasses this.

---

## Anti-Spin Rules

1. **Do not cite any "live Turtle-only" equity number until T38-FINAL exports it from the live-compatible harness.**
2. VOL_LOOKBACK is defined in config.rs as VL=8 (T37: VL=96 same-harness artifact rejected).
3. No more hyperopts on settled parameters (ATR_EMA, ATR_ENTRY_MULT, FRESHNESS_COOLDOWN, HOLD_MAX, CHAND_MULT confirmed 2-3× each).
4. If blocked on credentials, say so plainly.
5. T38 partial (live_compatible_wf.rs) is credible but incomplete — no equity export, no Base5 breakdown, VOL mismatch vs progress harness.
6. **Max 2 sequential optimizations per harness before mandatory held-out validation.** (AP=64 was #3 → REJECTED.)
7. **DDBudget Sharpe is milestone-aggregated — never compare directly to daily-equity Sharpe numbers.**
8. **ATR_RANK=24 is a candidate pending held-out validation — NOT a production default.** Same-harness sequential optimization pattern (EP=24 lesson applies). Run T42-held-out before any promotion.

---

## Anti-Overfit Rules

1. Minimum 3-window (5.5%) improvement on OOS before accepting any param change.
2. No sequential optimization on same data (EP=24 lesson — applies to AP=64).
3. Never re-run confirmed params at higher resolution on the same harness (VL=96 lesson).
4. Held-out validation required for marginal wins (< 3 windows over baseline).
5. Equity curve dominance required (>80% of time bars).
6. **Max 2 sequential optimizations per harness** before mandatory held-out validation.
7. **Same-harness artifact check:** If 3+ params were optimized on the same harness in sequence, the latest param needs held-out validation before trust.
8. **Before promoting any strategy to production default, verify it beats current default on pass rate AND return.**

---

## Graveyard / Rejections

| Strategy | Result | Key Reason |
|---|---|---|
| T40 Regime-Adaptive Exit | REJECTED | baseline M=2.30 wins all configs (2026-05-02) |
| S6 close_losers I=5 | GRAVEYARD | Incompatible with Turtle-only live exit (0 trades). Dual-exit only. |
| Donchian sleeve | REJECTED | 34/54 pass (63%) < guardrail 69.1%. |
| ATR_ENTRY_MULT=0.94 | REJECTED | Held-out 10/18 vs baseline 11/18. Anti-overfit discipline. |
| VOL_LOOKBACK=96 | REJECTED | Same-harness artifact (EP=24 pattern). Keep VL=8. |
| Mid-caps | REJECTED | 60% pass < 70% threshold. |
| ATR-norm position sizing | REJECTED | Equal capital optimal; ATR-norm inverts vol ranking. |
| Asymmetric exit | REJECTED | All configs identical to baseline. |
| Position scaling overlays | GRAVEYARD | All failed — Chandelier already manages it. |
| 4h Multi-Timeframe Turtle | GRAVEYARD | 1/20 pass — structural timeframe incompatibility. |
| BollingerReversion | GRAVEYARD | 0/288 OOS — signal actively harmful. |
| REGIME_ATR_PERIOD=64 | REJECTED | Same-harness artifact (sequential optimization #3 on live_compatible_wf.rs). |
| VPIN crash-state overlay | GRAVEYARD | Signal-to-noise too low for practical use. |
| OFI proxy | GRAVEYARD | Wrong normalization; abandoned. |
| 1h/4h Mean Reversion | GRAVEYARD | All symbols negative Sharpe on full history. |

---

## Unbuilt High-Priority Ideas (Pipeline)

| Idea | Priority | Status |
|---|---|---|
| LOB NOBI signal test | HIGH | Data collected, one harness run away |
| BTC-ETH cointegration | HIGH | First credible mean-reversion, never built |
| ETF flow institutional signal | HIGH | Data publicly available, never collected |
| Short-side sleeve | HIGH | 100% long book gap, proposed 2026-04-05 |
| DXY-Realized-Vol regime gate | MEDIUM | BTC now liquidity-sensitive risk asset |
| LOB NOBI daily aggregate (arxiv 2602.00776) | HIGH | Market-cap normalized, Binance public endpoint |
| Stablecoin exchange reserve state | MEDIUM | Binance public API, no auth required |
| Leverage-fragility state (Oct 2025 mechanics) | MEDIUM | Funding data in cache, proxy bug needs fix |