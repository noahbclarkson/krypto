# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-02 00:21 UTC. Research loop in documentation-spiral mode. 3 sessions without new strategy work. T40 (RAE) and T41 (stale report fix) are immediate priorities. Live testnet BLOCKED 5+ weeks on API keys.**

---

## Progress
- Turtle+Chandelier equity (daily, honest): **221.5x / Sharpe 1.04** (live_compatible_wf harness, VL=8, ATR_RANK=24)
- Turtle+ATR_RANK=24 equity: **86.1x / Sharpe 1.01** (progress_equity_curves harness)
- Live bot WF (ATR_RANK=24, Turtle-only): **52/63 pass (82.5%), Sharpe 5.590, +132.3% avg return** ✅
- `live_compatible_wf.rs`: ATR_RANK=24 in code ✅ — BUT `snapshots/live_compatible_wf.md` is STALE (documents T=5, not T=24)
- Live testnet: BLOCKED on Noah's API keys (5+ weeks)

---

## Current Truth

- `src/live/bot.rs` live path is **Turtle-only exit**; no Chandelier exit in deployed bot.
- ATR_RANK=24: live bot validated 52/63 pass (82.5%), Sharpe 5.590, +132.3% avg ret
- VOL_LOOKBACK=8 production default. VL=96 UNCONFIRMED (same-harness artifact risk — EP=24 pattern).
- Research loop in documentation-spiral: 3 consecutive sessions (5 commits) with no new strategy work — only docs/infrastructure fixes.

---

## Production Params (Frozen)

```text
EP                  = 21
TURTLE_ATR_P        = 24
TURTLE_ATR_M        = 2.0
HOLD_MAX            = 12
POSITION_CAP        = 3
FRESHNESS_COOLDOWN  = 0
ATR_ENTRY_MULT      = 0.00
REGIME_ATR_P        = 12
REGIME_LOOKBACK     = 42
ATR_RANK_THRESHOLD  = 24
VOL_LOOKBACK        = 8
```

---

## Next Tasks (Priority Order)

### T41: Regenerate live_compatible_wf.md — IMMEDIATE (documentation debt)
**Status:** STALE. `snapshots/live_compatible_wf.md` documents `T=5` but the code (`examples/live_compatible_wf.rs` line 36) has `T=24`. The actual run used T=24 (52/63 pass, Sharpe 5.590). The report is misleading. Must regenerate.
**Action:** Run `cargo run --example live_compatible_wf --profile sweep` → confirm T=24 outputs match the report → if report stale, rebuild it from harness output.

### T37: VL=96 vs VL=8 Base5-only Confirmation — HIGH
**Status:** UNCONFIRMED. VL=96 found on same harness that produced VL=8 (EP=24 artifact pattern). Run `live_compatible_wf.rs` on Base5 only (7 windows) with VL=96 vs VL=8 head-to-head. If VL=96 wins: promote. If not: delete VL=96 claim from all files.
**Note:** T41 must complete first so the harness state is clean before running.

### T40: Regime-Adaptive Exit (RAE) — HIGH (genuinely novel)
**Status:** UNBUILT. Mechanism differs from prior failed vol-contingent attempt (GRAVEYARD: uniform multiplier → all configs identical). RAE proposes CONDITIONAL switching: high-vol → M×1.1 (looser), low-vol → M×0.9 (tighter), neutral → M=2.30. This tests whether the vol regime changes the optimal exit multiplier, not just the absolute level.
**Build:** `examples/regime_adaptive_exit_walkforward.rs` — 3×3 grid of high/low multiplier pairs × 9 universes × 6 windows.
**Reject if:** No improvement over fixed M=2.30.
**This is the highest-value untested mechanism.** T40 was identified in strategy-ideas.md 3 sessions ago and never built.

### T42: ATR_ENTRY_MULT=0.94 Pre-2026 Held-Out Validation — MEDIUM
**Status:** UNRUN. Candidate (42/54 pass, Sharpe 5.34 vs baseline 40/54/3.15) found 2026-04-29 on the same WF grid. Anti-overfit correctly kept EM=0.00. But the candidate is strong enough to warrant a clean held-out test on pre-2026 data only.
**Build:** `examples/atr_entry_mult_heldout.rs` — test EM=0.00 vs EM=0.94 on pre-2026 windows (2018-2025 holdout). If EM=0.94 wins held-out: promote to candidate. If it loses: remove the claim from all files.

### T43: Mid-Cap Re-Test on Base5+LargeCaps5 Only — MEDIUM
**Status:** UNBUILT. Prior rejection (60% global pass < 70% threshold) was dragged down by legacy assets (LTC/EOS/BCH). Base5+LargeCaps5 = 12 windows with the 6 most liquid pairs. If 10+/12 pass: mid-cap expansion is viable for production universe.
**Build:** `examples/midcap_base5_largecaps_wf.rs` — Base5 + LargeCaps5 × 6 windows.

### T9: Live Testnet — CRITICAL BLOCKER
**Status:** BLOCKED on Noah's Binance testnet API key + secret. 5+ weeks blocked. All metrics remain simulation upper bounds.

---

## Anti-Spin Rules

1. ~~Do not cite any "live Turtle-only" equity number until T38-FINAL exports it from the live-compatible harness.~~ **T38 COMPLETE.**
2. ~~VOL_LOOKBACK is undefined in config.rs~~ — now defined (VL=8 in config.rs ✅ and in progress_equity_curves.rs ✅).
3. Do not promote VL=96 until T37 Base5-only confirmation.
4. No more hyperopts on settled parameters (ATR_EMA, ATR_ENTRY_MULT, FRESHNESS_COOLDOWN, HOLD_MAX, CHAND_MULT confirmed 2-3× each).
5. If blocked on credentials, say so plainly.
6. **Do not close research loop — it's in documentation-spiral mode.** T40/T42/T43 are genuinely untested mechanisms, not re-confirmations. Build them before declaring closure again.
7. No hyperopts on already-settled params to "comprehensively confirm" them. Same-harness re-running is a confirmation spiral, not research.

---

## Anti-Overfit Rules

1. Minimum 3-window (5.5%) improvement on OOS before accepting any param change.
2. No sequential optimization on same data.
3. Held-out validation required for marginal wins (< 3 windows over baseline).
4. Equity curve dominance required (>80% of time bars).
5. Never re-run confirmed params at higher resolution on the same harness. VL=96 is a violation of this rule.

---

## Graveyard / Rejections

| Strategy | Result | Key Reason |
|---|---|---|
| S6 close_losers I=5 | GRAVEYARD | Incompatible with Turtle-only live exit (0 trades). Dual-exit only. |
| Donchian sleeve | REJECTED | 34/54 pass (63%) < guardrail 69.1%. |
| ATR_ENTRY_MULT=0.94 | REJECTED (candidate pending T42) | Held-out 10/18 vs baseline 11/18. Pending T42 pre-2026 held-out validation. |
| VOL_LOOKBACK=96 | UNCONFIRMED | Same-harness artifact risk (EP=24 pattern). Keep VL=8 until T37 confirms. |
| Mid-caps (global) | REJECTED (pending T43) | 60% pass < 70% threshold. Re-test on Base5+LargeCaps5 only (T43). |
| ATR-norm position sizing | REJECTED | Equal capital optimal; ATR-norm inverts vol ranking. |
| Asymmetric exit | REJECTED | All configs identical to baseline. |
| Position scaling overlays | GRAVEYARD | All failed — Chandelier already manages it. |
| 4h Multi-Timeframe Turtle | GRAVEYARD | 1/20 pass — structural timeframe incompatibility. |
| BollingerReversion | GRAVEYARD | 0/288 OOS — signal actively harmful. |
| Vol-contingent Chandelier (uniform multiplier) | GRAVEYARD | All configs identical — uniform multiplier doesn't change behavior |
| ATR entry × volume confirmation | REJECTED | 40 configs, all inferior to no filter |
| A/D static sleeve | REJECTED | Below-random win rate, -6.2% vs Turtle |
| CTREND 25% fixed sleeve | REJECTED | Sharpe destroyed 1.38→0.33 |
| Equity integration | REJECTED | Combined Sharpe 1.05 vs crypto-only 4.00 |