# Hyperopt — ATR_ENTRY_MULT on Production Engine
**Date:** 2026-04-21 03:00 UTC
**Mission:** Re-validate ATR_ENTRY_MULT filter with current production params

## Context
Prior sweep (2026-04-13) tested ATR_ENTRY_MULT on **stale engine** (P=15/M=1.50/EP=21) and found M=0.0 winner. Since then, CHAND_PERIOD=11 and CHAND_MULT=2.25 have replaced P=15/M=1.50, and EP=24 replaced EP=21. The ATR entry filter mechanism is: only enter Turtle breakout if `close >= breakout_level + ATR × mult`.

**Key question:** Does ATR_ENTRY_MULT still not help with the tighter Chandelier exit engine?

## Method
- **Harness:** `examples/atr_entry_mult_prod_sweep.rs`
- **Params:** CHAND(11,2.25)/EP=24/ATR(24,2.0)/HM=12 — current production
- **Values:** 11 fine-grained mults ∈ {0.0, 0.05, 0.1, 0.15, 0.2, 0.25, 0.3, 0.4, 0.5, 0.75, 1.0}
- **Universes:** 9 universes × ~6 windows × 54 total window-runs
- **Output:** 595 window-run rows + 14,715 equity curve rows
- **Runtime:** 8.6 seconds

## Results

| mult | avg_sharpe | avg_ret% | pass | pass% |
|------|-----------|----------|------|-------|
| 0.00 | 1.2287 | 30.83 | 23/54 | 42.6% |
| 0.05 | 1.9122 | 40.29 | 29/54 | 53.7% |
| 0.10 | 1.6507 | 39.71 | 25/54 | 46.3% |
| 0.15 | 1.6837 | 44.67 | 28/54 | 51.9% |
| 0.20 | 1.6710 | 44.43 | 29/54 | 53.7% |
| 0.25 | 2.2187 | 59.76 | 33/54 | 61.1% |
| 0.30 | 1.9684 | 56.17 | 32/54 | 59.3% |
| 0.40 | 2.3028 | 78.12 | 30/54 | 55.6% |
| 0.50 | 2.2762 | 73.04 | 31/54 | 57.4% |
| 0.75 | 2.2505 | 50.57 | 34/54 | 63.0% |
| **1.00** | **2.6198** | **69.60** | **44/54** | **81.5%** |

**WINNER: ATR_ENTRY_MULT = 1.00**
- Sharpe: 2.6198 (+113% vs baseline M=0.0 at 1.2287)
- Pass rate: 81.5% (+38.9pp vs baseline 42.6%)
- Avg return: 69.6% (+38.8pp vs baseline 30.8%)
- Consistent winner across Base5, Legacy5BNB, OldGuardNoBNB, TopVolume5

## Mechanism Explanation
With the tighter Chandelier(P=11, M=2.25) exit (~fires bar 12-15):
- M=0.0: enter on any Turtle breakout → trades fire in volatile choppy conditions, Chandelier stops out quickly
- M=1.0: enter only when price is 1×ATR above the breakout level → requires strong momentum confirmation, reduces false breakouts, more winners survive the Chandelier exit

**The ATR entry filter is genuinely useful with the tighter Chandelier exit** — it acts as a momentum confirmation filter that reduces whipsaws.

## Interpretation
The prior sweep on stale engine (P=15/M=1.50) concluded ATR filter was useless because the loose Chandelier exit already filtered noise — the additional ATR filter provided no marginal benefit.

With the current tight Chandelier(P=11, M=2.25), the entry quality matters MORE because the exit is tighter and more reactive. ATR_ENTRY_MULT=1.0 acts as a genuine momentum filter.

## Files
- `examples/atr_entry_mult_prod_sweep.rs` — harness
- `snapshots/atr_entry_mult_prod.csv` — per-window metrics (595 rows)
- `snapshots/atr_entry_mult_equity.csv` — equity curves (14,715 rows)
- `snapshots/atr_entry_mult_summary.csv` — aggregated summary
- `charts/atr_entry_mult_comparison.png` — comparison chart

## Decision
**Do NOT update production default** — the 81.5% pass rate is impressive but:
1. Prior sweep on stale engine said M=0.0 was winner
2. Only 54 window-runs — need more validation before committing
3. Freshness cooldown = 0 means re-entry is immediate; ATR filter M=1.0 with fresh cooldown might skip reversals
4. Keep as **candidate** for live paper trading with ATR_ENTRY_MULT=1.0 alongside live defaults

## Recommended Next Steps
1. Run extended walk-forward with M=1.0 on testnet paper trading
2. Test M=1.0 specifically on bear market windows — does it still pass?
3. Consider: ATR_ENTRY_MULT=1.0 + FRESHNESS_COOLDOWN=1 (1 bar cooldown between re-entries)