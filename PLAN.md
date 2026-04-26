# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-26 18:35 UTC. S6 DONE — Chandelier REDUNDANT. Turtle-only is NON-INFERIOR. W05 live failure still UNEXPLAINED. Live testnet BLOCKED on API keys. Turtle-only (ATR 24, 2.0) is the sole validated production exit.**

---

## Brutal Self-Assessment (2026-04-26 Critique)

**What we got right:**
- Anti-overfitting: EP=24 + ATR_EM=0.85 properly rejected for in-sample inflation.
- T10 HOF script: code is truth, not markdown.
- Honest equity Sharpe (~1.29) vs inflated walk-forward Sharpe (6.29).
- Cross-market audit: edge generalizes to SPY/GLD/QQQ (Sharpe 0.76–0.87). Not crypto survivorship bias.
- MIN_TRADES threshold never binds (natural trade rate ~15-18/window).

**Where we're fooling ourselves:**
- 83% global pass = bull-era weighted. Pre-2021 stress = 67.9%.
- **W05 live failure (-22.7% YTD vs BTC +12.7%) is the real story.** Strategy is losing in current live regime. No explanation exists.
- No diversification: all eggs in Turtle+Chandelier. No choppy-regime strategy.
- All structural attempts to fix chop (position scaling, vol-adaptive Chandelier, 4h timeframe, correlation filter) FAILED or tied.
- CTREND portfolio never tested with CURRENT production params (T6 used stale params).
- Documentation loop: last 5 commits, 3/5 are docs/hygiene. Nothing new shipped since S4 GRAVEYARD.

**What needs to change:**
- Must have a SEPARATE strategy for choppy regimes, not a modification of Turtle+Chandelier.
- Metrics are inconsistent (DDBudget milestone vs Turtle daily equity — NOT comparable).
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

### S7: Turtle + CTREND Portfolio with Current Params — MEDIUM
**Concept:** Test Turtle(75%) + CTREND(EMA8/32, hold=30)(25%) using CURRENT production params.
**Why this matters:** T6 (2026-04-25) used STALE params (P=15, M=1.50, EP=21) and found Sharpe destroyed (1.38→0.33). Current params might produce a different result. CTREND fixed-hold (73% pass) is genuinely uncorrelated — different entry mechanics.
**Note:** Turtle-only is now the sole exit. CTREND portfolio test should use Turtle-only for both sleeves.
**Status:** Untested with current production params.

### S8: Donchian Entry vs Turtle Entry — MEDIUM
**Concept:** Donchian entry: `close > max(high, EP)` vs Turtle: `close > max(close, high, EP)`.
**Hypothesis:** Donchian requires close above highest high ever (stricter). Might produce fewer but higher-quality signals.
**Why this matters:** Entry signal space is NOT fully explored. We only tested Turtle variants.
**Test:** Walk-forward on Base5 (6 windows), Donchian vs Turtle with Turtle-only (ATR 24, 2.0) exit.
**Status:** Never tested.

### S9: W05 Live Failure Diagnostic — CRITICAL
**Concept:** Run W05 historical data through current production strategy to understand why it's losing.
**Why this matters:** -22.7% YTD vs BTC +12.7% is the #1 unresolved problem. Need to identify whether this is:
  (a) A regime-specific failure (chop/whipsaw, expected)
  (b) A data/API bug
  (c) A parameter issue
**Status:** Never diagnosed. Highest priority after S7/S8.

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

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **W05 live failure — no explanation** | CRITICAL | Strategy losing live. No fix identified. |
| **No choppy-regime strategy** | CRITICAL | Turtle+Chandelier requires trending markets. Nothing for chop. |
| **Metrics inconsistency** | HIGH | DDBudget Sharpe is milestone-aggregated, not comparable to equity Sharpe |
| **Turtle-only exit untested** | ~~HIGH~~ → DONE ✅ | S6: NON-INFERIOR, 36/54 pass, avg Sharpe 4.12 |
| **CTREND portfolio never tested with current params** | MEDIUM | T6 used stale params. Real result unknown with current production. |
| **Entry signal space unexplored** | MEDIUM | Donchian, other variants untested |
| **No live testnet** | CRITICAL | Still blocked on Noah's API keys. Cannot validate any of this. |

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
