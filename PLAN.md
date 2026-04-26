# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-27 20:05 UTC. T14 (live bot audit) — CRITICAL. S8 (Donchian entry) — UNTESTED. S9 (W05 diagnostic) — STILL UNDONE after 2+ weeks. Live testnet BLOCKED on API keys.**

---

## Brutal Self-Assessment (2026-04-27 Critique)

**What we got right:**
- Anti-overfitting: EP=24 + ATR_EM=0.85 properly rejected for in-sample inflation.
- T10 HOF script: code is truth, not markdown.
- Honest equity Sharpe (~1.29) vs inflated walk-forward Sharpe (6.29).
- Base5 (production universe) is clean: 6/6 pass.
- S6 proved Chandelier redundant via proper walk-forward comparison.
- Cross-market edge is REAL (SPY✓ GLD✓ QQQ✓) — though weaker than previously claimed (61% pass, QQQ fails individually at 58%).

**Where we're fooling ourselves:**
- **True global pass rate is ~67% (S6 head-to-head), not 83%.** The 83% figure is from an earlier stale-param sweep.
- **W05 live failure (-22.7% YTD vs BTC +12.7%) is the #1 unresolved problem.** Unexplained for 2+ weeks.
- **S6 result not deployed to live code.** The harness proved Turtle-Only non-inferior but `live_turtle_chandelier.rs` still has Chandelier logic.
- VOL_LOOKBACK hyperopt changes the walk-forward harness ranking only — zero impact on live trading. Spent a cycle on a harness-only parameter.
- Documentation loop: 3/5 last commits are docs/hygiene. The project documents itself faster than it builds.
- All choppy-regime fixes FAILED (10+ approaches). The problem is structural to Turtle+Chandelier.

**What needs to change:**
- Must audit live bot code — does it match validated harness?
- Must have a SEPARATE strategy for choppy regimes (CTREND regime-conditional switching is the best untested candidate).
- Research loop cannot be declared "closed" while W05 live failure is unresolved.

---

## Production Params (FROZEN — all validated, do NOT re-sweep)

```
EP              = 21     // ✅ held-out confirmed (T3): 27/29 pass
TURTLE_ATR_P    = 24     // ✅ confirmed 3×
TURTLE_ATR_M    = 2.0    // ✅ S6 confirmed: sole exit, Chandelier redundant
HOLD_MAX        = 12     // ✅ +71.4% Sharpe vs HM=45
ATR_ENTRY_MULT  = 0.00   // ✅ held-out confirmed
ATR_PERIOD      = 24     // ✅ confirmed 3×
POSITION_CAP    = 3
FRESHNESS_COOLDOWN = 0
```

**CHAND_PERIOD and CHAND_MULT are REDUNDANT.** S6 confirmed Chandelier(P=7, M=2.30) fires first in 0/54 tested windows. Turtle ATR(24, 2.0) is the sole validated exit. Remove Chandelier from production config.

**DO NOT re-sweep these params. They are confirmed.**

---

## Next 3 Execution Tasks

### T14: Live Bot Code Audit + Chandelier Removal — CRITICAL
**Concept:** Verify `live_turtle_chandelier.rs` matches S6 conclusion. Does it still have Chandelier dual-exit logic? S6 proved Chandelier fires first in 0/54 windows. Turtle ATR(24, 2.0) is the sole validated exit. Live code may still have Chandelier — needs audit and cleanup.
**Why this matters:** S6 was a backtest harness comparison, not a live code change. The validated walk-forward harness uses Turtle-Only, but we never verified `live_turtle_chandelier.rs` matches. Gap between validated theory and deployed code.
**Status:** Never audited. Highest priority.

### S8: Donchian Entry vs Turtle Entry — HIGH
**Concept:** Donchian entry: `close > highest_high_ever` vs Turtle: `close > max(high, close)_21bar`. Donchian is the original Richard Dennis 1983 entry — strictly tighter than Turtle (requires breakout above all-time high, not just 21-bar max).
**Why this matters:** Entry signal space is almost completely unexplored. We've spent all time on exit optimization. Entry is the other half of the problem. Donchian might produce fewer but higher-quality signals.
**Test:** Walk-forward on Base5 (6 windows), Donchian vs Turtle, Turtle ATR(24, 2.0) as sole exit.
**Status:** Never tested. Genuinely novel.

### S9: W05 Live Failure Diagnostic — CRITICAL
**Concept:** Run W05 (2024-10 to 2025-04, 2026 YTD equivalent) through current production strategy. Decompose every losing trade. Is it whipsaw chopt, bad entries, wrong exit timing, or a data/API issue?
**Why this matters:** -22.7% YTD vs BTC +12.7% is 2+ weeks unexplained. BTC is UP 12.7% in the same period. Strategy losing in a nominally bull market = whipsaw in chop. Must identify the mechanism before we can fix it.
**Status:** Never done. #1 unresolved problem in the project.

---

## T11: Failure Mode Diagnostic — DONE ✅
**Result:** All 13/54 failing windows are asset-specific (LTC/EOS/BCH only). Production universe (BTC/ETH/SOL/XRP/DOGE) is clean. W05 live failure is NOT a regime-wide failure — it's the specific choppy/bear character of 2026 YTD that Turtle+Chandelier cannot handle.

---

## S4: Vol-Adaptive Chandelier — GRAVEYARD ✅
**Result:** 252-bar vol rank tied (Δ+0.12 Sharpe, noise). No adaptive benefit. GRAVEYARD.

---

## S6: Turtle-Only Exit Test — DONE ✅ NON-INFERIOR
**Result:** Turtle-only (ATR 24, 2.0) produces 36/54 pass, avg Sharpe 4.12 vs Turtle+Chandelier 36/54, Sharpe 3.48.
**Key findings:**
- Chandelier(P=7, M=2.30) fires FIRST in 0/54 tested windows. Turtle ATR(24, 2.0) is the dominant exit.
- Same pass rate (36/54), higher Sharpe (+0.64), 70 fewer trades (Turtle ATR is slightly tighter).
- Universes with net benefit from S6: Legacy4(+1 pass), Legacy3(+1 pass), LowVolume5(+1 pass), OldGuard4(+1 pass).
- Universes with net harm from S6: NoDOGE(-1), OldGuardNoBNB(-1), LargeCaps5(-2).
- **Action:** Remove Chandelier from production config. Turtle-only is the production exit.

---

## Stop Doing

- **Re-sweeping confirmed params** — hard stop. EP=21, P=7, M=2.30, HM=12, ATR=24 all confirmed.
- **Documentation-only sprints** — last 5 commits: 3/5 docs. Move the project or stand still.
- **Claiming research loop is "closed"** — W05 live failure is unresolved. The loop is not closed.
- **Re-adding Chandelier** — S6 confirmed it adds zero value.
- **Harness-only parameter tuning** — VOL_LOOKBACK is a walk-forward ranking param, not a production param. It doesn't affect live trading. Don't spend cycles on it.
- **Fixed CTREND portfolio sleeve (S7)** — Fixed 25/75 blend destroys Sharpe. Regime-conditional switching is the viable variant (untested).

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **W05 live failure — no explanation** | CRITICAL | 2+ weeks, no diagnostic run. BTC up +12.7%, strategy down -22.7%. |
| **Live bot still has Chandelier?** | CRITICAL | S6 was harness-only. `live_turtle_chandelier.rs` not verified. |
| **True global pass rate** | HIGH | ~67% (S6 head-to-head) vs claimed 83% (stale-param sweep) |
| **No choppy-regime strategy** | CRITICAL | 10+ approaches failed. CTREND reg-conditional switching untested. |
| **Entry signal space unexplored** | MEDIUM | Donchian entry, other variants never tested |
| **VOL_LOOKBACK = harness only** | LOW | Zero live impact. Don't spend cycles on it. |
| **Cross-market edge marginal** | MEDIUM | QQQ individually fails (58% < 60%). Overall 61% (just above threshold). |
| **No live testnet** | CRITICAL | Blocked on Noah's API keys. Nothing else matters until unblocked. |

---

## Graveyard Summary

All strategies below confirmed dead:

| Strategy | Result | Key Reason |
|----------|--------|------------|
| EP=24 | REVERTED | In-sample inflation, failed held-out |
| ATR_ENTRY_MULT=0.85 | REVERTED | In-sample inflation, failed held-out |
| CP=42 | REJECTED | Backward search artifact |
| ATR_ENTRY_MULT>0 | REJECTED | All values degrade pass rate |
| ATR_PERIOD re-sweep | NULL | ATR=24 confirmed 3× |
| CHAND_MULT re-sweep | NULL | M=2.30 confirmed 2× |
| CHAND_PERIOD re-sweep | NULL | P=7 confirmed — but Chandelier is REDUNDANT (S6) |
| 4h multi-timeframe | GRAVEYARD | Structural failure (1/20 pass) |
| Cross-market equity integration | REJECTED | Combined -2.94 Sharpe vs crypto-only |
| DynamicTrend EMA signal | REJECTED | Turtle wins 21/24 windows |
| BollingerReversion | GRAVEYARD | 0/288 OOS pass |
| BOCPD regime detector | GRAVEYARD | 0% breaks |
| FDUSD basis carry | GRAVEYARD | 19% pass |
| Funding rate MR | GRAVEYARD | 43% pass |
| Vol-contingent Chandelier (21-bar) | GRAVEYARD | All configs identical |
| Vol-contingent Chandelier (252-bar) | GRAVEYARD | Tied, no benefit |
| Position scaling overlays | GRAVEYARD | All failed |
| Regime switching | GRAVEYARD | All failed |
| CTREND + Chandelier exit | REJECTED | 30/54 pass |
| CTREND fixed-hold | VIABLE | 73% pass, secondary signal only |
| Turtle ATR Entry Filter | REJECTED | mult=0.0 definitive winner |
| Correlation entry filter | REJECTED | Chandelier already handles it |
| Donchian entry | UNTESTED | S8 priority |
| Turtle-only exit | NON-INFERIOR ✅ | S6: Chandelier REDUNDANT, Turtle ATR is sole exit |

---

## Research Loop: What Remains

1. ~~**S6:**~~ ~~Turtle-only exit test~~ — DONE ✅ NON-INFERIOR
2. **S7:** Turtle + CTREND portfolio with current params — medium priority
3. **S8:** Donchian entry test — medium priority, independent
4. **S9:** W05 live failure diagnostic — CRITICAL, unexplained
5. **T9:** Live testnet — BLOCKED on Noah's API keys (nothing else matters until unblocked)
