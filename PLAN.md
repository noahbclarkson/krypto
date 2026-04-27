# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-27 19:52 UTC. CRITICAL: S17 Chandelier removal REVERTED by POSITION_CAP commit. Production bot still has Chandelier. S8 Donchian NEVER TESTED (marked done falsely). Live testnet BLOCKED on API keys.**

---

## Brutal Self-Assessment (2026-04-27 Late Critique)

**What we got right:**
- Anti-overfitting: EP=24 + ATR_EM=0.85 properly rejected for in-sample inflation.
- T10 HOF script: code is truth, not markdown.
- Honest equity Sharpe (~1.29) vs inflated walk-forward Sharpe (6.29).
- Base5 (production universe) is clean: 6/6 pass.
- S6/S17 proved Chandelier fires first 100% of trades and DEGRADES Sharpe (3.11 vs 4.58 Turtle-only).
- POSITION_CAP sweep correctly confirmed CAP=3 under dual-exit.

**Where we're fooling ourselves:**
- **S17 conclusion REVERTED:** `049b13ed` (POSITION_CAP sweep, 20 min after `e317a830`) overwrote `check_turtle_exit` back to `check_dual_exit`. Production bot has Chandelier. S17 walk-forward is not in HEAD.
- **S8 Donchian marked done, NEVER TESTED:** No harness, no chart, no snapshots. PLAN.md false documentation.
- **Production/harness strategy mismatch:** Bot uses dual-exit. S17 harness tested Turtle-only. Results from different strategies being compared.
- **Documentation loop:** 53% of all 250 commits are docs/hygiene. The project documents faster than it builds.
- **strategy-ideas.md stale:** Lists CTREND switching as "best untested candidate" — it FAILED at 67% pass.

**What needs to change:**
- Must actually deploy S17 conclusion (or explicitly revert it with justification)
- Must test S8 Donchian (genuinely untested)
- Must align harness and production code — same strategy
- Research loop NOT closed until live testnet works

---

## Production Params (FROZEN — all validated, do NOT re-sweep)

```
EP              = 21     // ✅ held-out confirmed (T3): 27/29 pass
TURTLE_ATR_P    = 24     // ✅ confirmed 3×
TURTLE_ATR_M    = 2.0    // ✅ S6/S17: sole exit, Chandelier redundant
HOLD_MAX        = 12     // ✅ +71.4% Sharpe vs HM=45
ATR_ENTRY_MULT  = 0.00   // ✅ held-out confirmed
ATR_PERIOD      = 24     // ✅ confirmed 3×
POSITION_CAP    = 3
FRESHNESS_COOLDOWN = 0
```

**CHAND_PERIOD and CHAND_MULT are REDUNDANT per S17.** S17 walk-forward with production params:
- Dual-exit (Turtle+Chandelier): 40/54 pass, Sharpe 3.11, 715 trades
- Turtle-only (ATR 24, 2.0): 39/54 pass, Sharpe 4.58, 658 trades
- Chandelier fires first: 336/336 (100%) — it's not a dual exit, it's a single Chandelier exit
- Turtle-only wins: +1.47 Sharpe, 57 fewer trades

**BUT: S17 conclusion NOT in production code.** `049b13ed` reverted it. Decision needed: deploy Turtle-only or keep Chandelier.

---

## Next 3 Execution Tasks

### T18: Deploy or Revert S17 Conclusion — CRITICAL (do first)
**Two options:**
- **Option A:** Actually remove Chandelier from `src/live/bot.rs` (deploy S17). Change `check_dual_exit` → `check_turtle_exit`, remove Chandelier trail calc. Re-run POSITION_CAP sweep under Turtle-only to confirm CAP=3 still holds.
- **Option B:** Keep Chandelier in production. Document why Chandelier is kept despite S17 walk-forward showing it degrades Sharpe by -1.47. The production bot logic would then match the harness that got CAP=3 confirmed.

**Why this matters:** The POSITION_CAP sweep was run with `USE_CHANDELIER=true` harness. CAP=3 is validated against dual-exit. If we go Turtle-only, CAP=3 must be re-confirmed under Turtle-only logic. This is the critical path.

**Commit `e317a830` (S17) vs `049b13ed` (POSITION_CAP) conflict:**
- S17: Turtle-only wins walk-forward by +1.47 Sharpe
- POSITION_CAP: confirmed CAP=3 under dual-exit (current production)
- Re-running POSITION_CAP under Turtle-only is the honest next step before switching

### S8: Donchian Entry vs Turtle Entry — GENUINELY UNTESTED
**Concept:** Turtle uses `close > max(close, high) [21-bar]`. Donchian uses `close > highest(high) [strictest — all-time high breakout]`.
**Why this matters:** Entry signal space is almost completely unexplored. All our work has been on exit optimization. Donchian might produce fewer but higher-quality signals.
**Test:** Walk-forward on Base5 (6 windows), Donchian entry vs Turtle entry, Turtle ATR(24, 2.0) as sole exit.
**Status:** Never tested. Not even the harness exists. S8 was falsely marked done in PLAN.md.
**Priority:** LOW (live testnet is more important). Do after T18.

### T9: Live Testnet — BLOCKED (nothing else matters until unblocked)
**Status:** BLOCKED on Noah's API keys. No parameter, no strategy, no code change matters until live testnet works.
**Blocker:** Need Noah to provide Binance testnet API keys.
**Escalation:** This has been blocked for multiple sessions. Nothing else I do advances the project until this is resolved.

---

## Stop Doing

- **Documentation-only sprints** — 53% of all commits are docs/hygiene. Move the project or stand still.
- **Marking S8 "done" without executing it** — this is false documentation
- **Re-sweeping confirmed params** — hard stop. EP=21, ATR=24, HM=12, CAP=3 all confirmed.
- **Harness-only parameter tuning** — VOL_LOOKBACK is a harness ranking param, zero impact on live trading.

---

## Blind Spots (Updated 2026-04-27)

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **S17 Chandelier removal REVERTED** | CRITICAL | `049b13ed` overwrote `e317a830`. Bot still has Chandelier. |
| **Production/harness strategy mismatch** | CRITICAL | Bot=dual-exit, S17 harness=Turtle-only. Results not comparable. |
| **S8 Donchian never tested** | HIGH | Marked done in PLAN.md, no harness exists |
| **strategy-ideas.md stale** | HIGH | Lists CTREND switching as best candidate — FAILED at 67% |
| **HALL_OF_FAME.md stale** | MEDIUM | Says "Turtle+Chandelier," lists CHAND params as active |
| **No live testnet** | CRITICAL | BLOCKED on Noah's API keys — project is stuck |
| **Anti-overfitting hole** | MEDIUM | "Never re-optimize confirmed params" has no enforcement |

---

## Graveyard Summary

All strategies confirmed dead:

| Strategy | Result | Key Reason |
|----------|--------|------------|
| EP=24 | REVERTED | In-sample inflation on same OOS data |
| EP=43 | REVERTED | Found in same session as EP=21 validation — same violation |
| ATR_ENTRY_MULT=0.85 | REVERTED | In-sample inflation, same session as EP=24/P=7 |
| ATR_ENTRY_MULT>0 | REJECTED | All non-zero values degrade pass rate |
| CHAND_PERIOD re-sweep | REDUNDANT | Chandelier fires first 100% of trades — remove it |
| 4h multi-timeframe | GRAVEYARD | Structural failure (1/20 pass) |
| Cross-market equity integration | REJECTED | Combined -2.94 Sharpe vs crypto-only |
| DynamicTrend EMA signal | REJECTED | Turtle wins 21/24 windows |
| BollingerReversion | GRAVEYARD | 0/288 OOS pass |
| BOCPD regime detector | GRAVEYARD | 0% breaks |
| FDUSD basis carry | GRAVEYARD | 19% pass |
| Funding rate MR | GRAVEYARD | 43% pass |
| Vol-contingent Chandelier | GRAVEYARD | All configs identical |
| Position scaling overlays | GRAVEYARD | All failed |
| CTREND regime-conditional switching | REJECTED | 67% pass < 70% threshold |
| Donchian entry | UNTESTED | Never built harness |
| Turtle-only exit | CONFIRMED ✅ | S17: +1.47 Sharpe better than dual-exit |

---

## Research Loop: What Remains

1. **T18:** Deploy or explicitly revert S17 Chandelier conclusion — CRITICAL, blocked on decision
2. **S8:** Donchian entry test — genuinely untested
3. **T9:** Live testnet — BLOCKED on Noah's API keys
4. **strategy-ideas.md:** Clean up stale entries (CTREND switching is DEAD)
