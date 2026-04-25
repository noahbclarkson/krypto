# PLAN.md — Krypto Research & Critique Cycle

**State: 2026-04-25 12:17 UTC. Critique complete. Research loop CLOSED. Parameter revert recommended. BLOCKED on live testnet (Noah's API keys).**

---

## ⚠️ CRITICAL: Parameter Revert Recommended

**Stop all hyperoptimization. Revert to more robust params.**

Recent "improvements" are noise-level (1-2 windows in 54-window tests):
- EP=24 vs EP=21: +2 windows (83.3% vs 79.6%) — noise
- ATR_ENTRY_MULT=0.85 vs 0.90: +1 window (81.5% vs 79.6%) — noise
- CHAND_MULT=2.30 vs 2.25: +1 window (83.3% vs 81.5%) — noise

**Recommended production params (revert to pre-noise defaults):**
```
EP = 21              (was 24 — revert)
ATR_ENTRY_MULT = 0.90  (was 0.85 — revert)
CHAND_MULT = 2.25    (was 2.30 — revert; M=2.30 only +1 window)
CHAND_PERIOD = 7     (keep — genuine improvement from P=11)
HOLD_MAX = 12       (keep — genuine improvement)
ATR_PERIOD = 24     (keep — validated)
POSITION_CAP = 3
FRESHNESS_COOLDOWN = 0
```

**Rationale:** A 1-2 window improvement in 54 windows is within the false-positive rate (~30% at 70% baseline). We have been optimizing the OOS validation set. The pre-revert params (EP=21, EM=0.90, M=2.25) were themselves validated OOS. Stop cycling.

---

## CRITICAL — Pending

### T3: Parameter Revert → Re-validate on Walk-Forward 🟡
- **Action:** Revert EP=21, EM=0.90, M=2.25 → run 9-universe walk-forward → confirm pass rate ≥ 80%
- **Why:** Recent "improvements" are 1-2 windows in 54. Revert to validated params.
- **Win condition:** Reverted params pass ≥ 80% global → confirmed as production.
- **Note:** Pre-2021 stress (20/20 pass) was with P=7/M=2.25/EP=24 — the reverted params (EP=21/M=2.25) have NOT been stressed on pre-2021 data. Run regime_stress_p7_current with EP=21 to confirm.

### T6: CTREND Fixed-Hold Exit Sweep 🟡
- **Signal confirmed genuine** (Monte Carlo: 0/500 shuffled beat real).
- **Prior result:** CTREND + Chandelier dual-exit = 30/54 pass (rejected). Wrong exit mechanism.
- **New test:** CTREND entry + fixed-hold sweep (10, 15, 21, 30, 45, 60, 90 bars). CTREND is multi-horizon momentum — may need longer hold, not tighter Chandelier.
- **Baseline:** Turtle+Chandelier = 43/54 pass (~80%). Any CTREND variant >40/54 is a viable signal family.
- **Win condition:** CTREND + fixed-hold > 35/54 AND genuinely different signal from Turtle (correlation < 0.7).
- **Why it matters:** Only untested idea producing genuinely different signal family, not parameter tuning.

### T7: BTC/ETH Correlation Entry Filter 🟡
- **Hypothesis:** 2026 YTD failure (-32.8% partial harness) may be BTC-led divergence. ALT breakouts without BTC confirmation get stopped by Chandelier. BTC/ETH trend filter might reduce whipsaw.
- **Test:** Turtle+Chandelier with BTC/ETH trend confirmation filter for ALT entries.
  - No filter (baseline)
  - BTC signal required for ALT entries
  - BTC OR ETH signal (any 1-of-2)
  - BTC AND ETH signal (both required)
- **Win condition:** Filter must improve pass rate OR Sharpe without reducing trade count by >30%.
- **Risk:** Every entry filter tested (ATR, volume, chop) hurt pass rate. Correlation filter is same mechanism class.

### T8: Live Execution Audit 🔴 BLOCKED
- **Cannot be backtested.** `live_execution_audit.rs` is built.
- **Known risk:** SOL slippage at $50K likely 3-5bp (vs 1bp model). FillLog CSV exists but no analysis harness.
- **Action:** When testnet connects → run audit harness on FillLog CSVs, compute per-symbol: actual slippage vs model, maker-fill rate, fee paid vs expected.
- **Alert rule:** If SOL actual > 2x model → reduce SOL cap.

---

## BLOCKED — Waiting on Noah

### T9: Live Testnet
Noah needs Binance testnet API keys. Without this, no live paper trading.
**This is the only remaining path to new knowledge.**

---

## Stop Doing

- **Hyperopt cycling on stable params** — EP, ATR, CHAND_M, ATR_ENTRY_MULT all reverted to robust defaults. Stop re-running.
- **Equity vanity numbers** — stop saying specific equity multiples (246x, 734x, 1048x). The same strategy with P=7 vs P=15 produced 246x vs 734x — a 3x difference from one param. Equity is unstable. Report pass rate and honest equity Sharpe (~1.0-1.3).
- **Re-running regime stress** — done, 20/20 pass.
- **Documentation cycling** — last 5 commits: 2 docs/audit, 2 marginal hyperopt, 1 correct rejection. Stop auditing ourselves.
- **DDBudget Sharpe 7.24** — this is the same inflated walk-forward methodology as Turtle's 6.29. Not comparable to equity Sharpe. If reported, must be clearly labeled as "walk-forward per-window averaged Sharpe (not equity Sharpe)."

---

## Production Params (REVERT RECOMMENDED — 2026-04-25)

```
EP=21, CHAND_PERIOD=7, CHAND_MULT=2.25, HOLD_MAX=12,
ATR_PERIOD=24, ATR_ENTRY_MULT=0.90, POSITION_CAP=3, FRESHNESS_COOLDOWN=0
```

**Source of truth: `examples/live_turtle_chandelier.rs`**
**NOTE:** This reverts EP (24→21), ATR_ENTRY_MULT (0.85→0.90), CHAND_MULT (2.30→2.25). CHAND_PERIOD=7 and HOLD_MAX=12 are retained (genuine improvements).

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **Recent param "improvements" are noise** | 🔴 CRITICAL | EP=24, EM=0.85, M=2.30 — each 1-2 windows in 54 = noise |
| **Live execution unknown** | 🔴 CRITICAL | BLOCKED on API keys |
| **CTREND fixed-hold exit** | 🟡 MEDIUM | Untested — genuinely different signal family |
| **Correlation entry filter** | 🟡 MEDIUM | Untested — 2026 failure hypothesis |
| **DDBudget vs Turtle metric comparability** | 🟡 MEDIUM | 7.24 vs 1.68 are different methodologies — not comparable |

---

## Graveyard Summary (Complete — as of prior sessions)

- All non-trend strategies: FAILED
- All regime switching: FAILED
- All entry-side filters: FAILED (ATR, volume, chop, correlation)
- Vol-rank overlays: FAILED
- Position scaling overlays: FAILED
- CTREND + Chandelier exit: FAILED (wrong exit mechanism)
- 4h multi-timeframe: FAILED (1/20 pass)
- Cross-market: SPY/GLD pass, QQQ marginal (61%)

---

## Research Loop: Truly Closed — Except Live Execution

Only T3 (param revert + re-validate), T6 (CTREND fixed-hold), T7 (correlation filter), T8 (live audit) and live testnet remain as valid work.

**Everything else has been tested to exhaustion or killed.**
