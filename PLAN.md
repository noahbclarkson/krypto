# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-02 20:06 UTC. Critique cycle complete. T42/T43/T44 identified. ATR_RANK=24 promotion overdue. LOB NOBI signal never tested. Documentation spiral confirmed (5/8 recent commits). Live testnet BLOCKED 5+ weeks.**

---

## Critique Findings (2026-05-02)

**Core judgment:** ATR_RANK=24 is our best validated strategy (82.5% pass, Sharpe 5.590, +132% OOS) and is NOT the production default. This is a promotion failure. The program is optimizing the same basin while leaving genuinely validated strategies on the bench.

**DDBudget Sharpe 7.20 is structurally misleading:** milestone-aggregated returns smooth drawdowns and inflate Sharpe vs daily-equity strategies. Turtle (0.98) and DDBudget (7.20) are not comparable. daily_progress.csv mixes these without methodology warnings.

**Equity metric instability:** Turtle equity collapsed from 734x (2026-04-20) → 124x → 110x purely from methodology corrections. HOF number is current truth but unstable.

**Biggest structural gaps (never touched):**
- LOB NOBI daemon running since 1862b57, signal never tested (lowest-lift highest-value)
- BTC-ETH cointegration pair trading (first credible mean-reversion, proposed 2026-04-06, zero commits)
- Short-side sleeve (100% long book, zero commits since proposed 2026-04-05)
- ETF flow institutional signal (proposed 2026-04-07, zero commits)

**Bear market gap:** Only ~12 months of bear OOS data (2022). 2018-2019 (90% BTC drawdown) not in any window. Strategies untested in sustained multi-year bear.

**Documentation spiral:** 5/8 recent commits are pure docs. Ideas generated 10x faster than tested.

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

### T42: Promote ATR_RANK=24 to production default
**Status:** READY — one config change, no new code.
- ATR_RANK=24: 52/63 pass (82.5%), Sharpe 5.590, +132.3% avg OOS return
- Turtle (current default): 34/54 pass (63%), Sharpe 1.00, 110x equity
- ATR_RANK=24 is better on every objective metric (pass rate, Sharpe, return)
- Update `src/live/config.rs`: ATR_RANK_THRESHOLD default → 24
- Update HALL_OF_FAME.md to reflect ATR_RANK=24 as the primary production strategy
- One commit. Immediate production value.

### T43: LOB NOBI Signal Test
**Status:** READY — data already collected, one harness run.
- LOB daemon running since commit 1862b57 — NOBI depth data in file
- Compute: daily top-5 depth imbalance `(bidQty-askQty)/(bidQty+askQty)` → SG smoothing → z-score signal
- Test: NOBI > threshold predicts next-24h directional continuation vs zero baseline
- If positive edge → build microstructure sleeve. If null → GRAVEYARD cleanly.
- Lowest-lift highest-value test in entire program history.

### T44: Mock Exchange (Bypass Live Testnet Blocker)
**Status:** UNBUILT. 5+ weeks blocked on API keys.
- Lightweight Rust HTTP mock for binance-rs-async endpoints
- Seed with historical 1m klines to simulate fills and slippage
- Would have caught the 2026-05-01 live exit bug before testnet
- Unblocks execution logic testing without credentials — infinite iteration speed
- Milestone: serve `/api/v3/klines` from historical data, return mock fills for market orders

### T45: BTC-ETH Cointegration Pair Trade
**Status:** PROPOSED. First credible mean-reversion in pipeline.
- Every prior mean-reversion attempt is GRAVEYARD: RSI, BollingerReversion, OFI, VPIN, 4h MR, 1h MR
- BTC-ETH has academic backing (Frontiers Jan 2026, cointegrating coefficient ~0.0587)
- Build rolling Johansen test + spread z-score for entry/exit
- Completely orthogonal to entire existing directional trend book
- Medium complexity — one new harness file

### T9: Live Testnet
**Status:** BLOCKED on Noah's Binance testnet API keys (5+ weeks). T44 bypasses this.

---

## Anti-Spin Rules

1. **Do not cite any "live Turtle-only" equity number until T38-FINAL exports it from the live-compatible harness.**
2. VOL_LOOKBACK is defined in config.rs as VL=8 (T37: VL=96 same-harness artifact rejected).
3. No more hyperopts on settled parameters (ATR_EMA, ATR_ENTRY_MULT, FRESHNESS_COOLDOWN, HOLD_MAX, CHAND_MULT confirmed 2-3× each).
4. If blocked on credentials, say so plainly.
5. T38 partial (live_compatible_wf.rs) is credible but incomplete — no equity export, no Base5 breakdown, VOL mismatch vs progress harness.
6. **Max 2 sequential optimizations per harness before mandatory held-out validation.** (AP=64 was #3 → REJECTED.)
7. **DDBudget Sharpe is milestone-aggregated — never compare directly to daily-equity Sharpe numbers.**
8. **ATR_RANK=24 must be promoted before further optimization on same basin.**

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