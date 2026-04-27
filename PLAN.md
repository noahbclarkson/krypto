# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-27 21:56 UTC. CRITICAL: All metrics are simulation upper bounds. Live testnet is the only path forward. S8 Donchian genuinely untested. POSITION_CAP validated for Turtle-only, not for dual-exit production bot.**

---

## Brutal Self-Assessment (2026-04-27 Evening Critique)

**What we got right:**
- Anti-overfitting: EP=24 + ATR_EM=0.85 properly rejected for in-sample inflation. Rules work.
- Equity Sharpe (~1.29) correctly reported as honest number. Walk-forward 5.46 never on charts.
- Graveyard is thorough — every failed strategy documented with reason.
- Cross-market validation: SPY/QQQ/GLD all pass at ~60%+. Strategy is not crypto-survivorship bias.
- Base5 (production universe): 6/6 pass. Failures are LTC/EOS/BCH (structural non-trending assets).
- Maker-fill mechanism documented: bar-close limit order, Post Only, ~70% maker rate estimated.

**Where we're fooling ourselves:**
- **POSITION_CAP harness tested Turtle-only, not production dual-exit.** The harness (`position_cap_hyperopt.rs`) has standalone Turtle-only exit logic. The production bot (`src/live/bot.rs`) uses `check_dual_exit` (Chandelier + Turtle ATR combined). CAP=3 is validated for a strategy the bot doesn't run. Likely holds but unconfirmed.
- **All metrics are upper bounds.** Walk-forward Sharpe 5.46, equity Sharpe 1.29, $10K→$67M — all simulation. The fee model, maker-fill rate, and slippage assumptions are unvalidated in live market conditions.
- **S8 Donchian marked done, never executed.** No harness, no chart, no snapshots.
- **Documentation loop is structural.** 53% of all commits are docs/hygiene. Nothing breaks this except live testnet or genuine new research.

---

## Production Params (FROZEN — all validated, do NOT re-sweep)

```
EP              = 21     // ✅ held-out confirmed
TURTLE_ATR_P    = 24     // ✅ confirmed 3×
TURTLE_ATR_M    = 2.0    // ✅ confirmed 2×
CHAND_PERIOD    = 7      // ✅ held-out confirmed
CHAND_MULT      = 2.30   // ✅ 71-value dense sweep
ATR_ENTRY_MULT  = 0.00   // ✅ no filter wins
HOLD_MAX        = 12     // ✅ HM=12 wins +71.4% Sharpe vs HM=45
POSITION_CAP    = 3      // ⚠️ validated for Turtle-only harness, not dual-exit bot
FRESHNESS_COOLDOWN = 0   // ✅ cd=0 wins
```

**Note on POSITION_CAP:** The CAP=3 confirmation (2026-04-27) was run on a Turtle-only ATR exit harness. The production bot uses `check_dual_exit` (both Chandelier AND Turtle ATR). CAP=3 likely holds for dual-exit (Turtle ATR is tighter most of the time), but this is an untested assumption. Re-validation under dual-exit would be the honest next step — but it matters only when live testnet is working.

---

## Next 3 Execution Tasks

### T19: Build Donchian Entry Harness + Walk-Forward (HIGH PRIORITY — genuinely untested)
**Concept:** Turtle uses `close > max(close, high) [21-bar]`. Donchian uses `close > highest(high) [strictest — all-time high breakout]`. The original 1983 Turtle system used Donchian. Fewer signals, potentially higher quality.
**Why this matters:** All our work has been on exit optimization. Entry signal space is almost completely unexplored. Every strategy comparison held entry constant. Donchian tests an alternative entry hypothesis.
**Test:** Walk-forward on Base5 (6 windows), Donchian entry vs Turtle entry, Turtle ATR(24, 2.0) as sole exit.
**Status:** Never tested. No harness exists. No chart. No snapshots.
**Priority:** HIGH. This is the single most promising genuinely untested idea.
**Execution:** Build `examples/donchian_walkforward.rs`, run 6-window Base5 walk-forward, compare to Turtle baseline.

### T20: ATR-Rank Conditional Entry Filter (NEW — genuinely untested)
**Concept:** Not a fixed ATR_MULT (failed at all values). Instead: only enter if current 21-bar ATR is above its 60th percentile in 252-bar history. High ATR = trending environment = valid Turtle setup. Low ATR = choppy = filter out.
**Why this matters:** Mechanistically different from ATR_MULT. ATR_MULT is a fixed threshold; ATR-rank is regime-dependent. In high-vol regimes (which trend), the threshold is automatically higher. In low-vol chop, it's automatically stricter.
**Test:** Sweep threshold {50th, 60th, 70th} percentile × Base5 walk-forward.
**Status:** Never tested. Genuinely novel.
**Priority:** MEDIUM (after Donchian).
**Execution:** Build harness, run sweep.

### T9: Live Testnet — CRITICAL BLOCKER
**Status:** BLOCKED on Noah's Binance testnet API keys.
**Everything else is secondary.** The project cannot make forward progress without live market validation. All metrics are upper bounds. Fee model, maker-fill rate, slippage — all unvalidated.
**Escalation:** This has been blocked for weeks. Nothing advances the project until this is resolved.
**What we need:** Binance testnet API key + secret. Not production keys — testnet only.

---

## Stop Doing

- **Re-sweeping confirmed params.** EP=21, ATR=24, M=2.0, CHAND_P=7, CHAND_M=2.30, HOLD_MAX=12, ATR_ENTRY_MULT=0.00 — all confirmed. Stop.
- **Marking things "done" without execution.** S8 Donchian was marked done in PLAN.md. It wasn't.
- **Using walk-forward Sharpe 5.46 in external communications.** Only equity Sharpe ~1.29 is honest.
- **Building documentation-only commits.** The documentation loop is structural. Only live testnet or genuine new research breaks it.

---

## Blind Spots (Updated 2026-04-27 Evening)

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **No live testnet** | CRITICAL | BLOCKED on Noah's API keys — nothing else matters |
| **POSITION_CAP tested for wrong strategy** | HIGH | Harness=Turtle-only, Bot=dual-exit. CAP=3 likely holds but unconfirmed |
| **S8 Donchian never tested** | HIGH | Marked done, actually untested |
| **All metrics are upper bounds** | HIGH | Sharpe, returns, drawdowns — all simulation maximums |
| **Maker-fill rate unvalidated** | MEDIUM | 70% estimated, unknown in live bear/volatile conditions |
| **Bot/harness code split** | MEDIUM | Separate exit logic implementations — can diverge silently |
| **Documentation loop** | MEDIUM | Structural — only live testnet or new research breaks it |

---

## Graveyard Summary (Updated 2026-04-27)

All strategies confirmed dead:

| Strategy | Result | Key Reason |
|----------|--------|------------|
| EP=24 | REVERTED | In-sample inflation on same OOS data as P=7 |
| EP=43 | REVERTED | Same session violation as EP=21 validation |
| ATR_ENTRY_MULT>0 | REJECTED | All non-zero values degrade pass rate |
| CHAND_PERIOD re-sweep | REDUNDANT | Chandelier fires first 100% — remove it |
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
| ATR rank conditional filter | UNTESTED | NEW — not yet built |
| Donchian entry | UNTESTED | S8 — never built harness |

---

## Research Loop: What Remains

1. **T19:** Donchian entry walk-forward — genuinely untested, highest priority
2. **T20:** ATR-rank conditional entry filter — genuinely untested, novel approach
3. **T9:** Live testnet — BLOCKED on Noah's API keys. The only thing that validates everything.

**Note:** The research loop cannot "close" in the traditional sense because all metrics are simulation. The loop closes only when live testnet provides real market feedback.