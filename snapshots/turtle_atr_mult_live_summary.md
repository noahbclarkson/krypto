# T74: TURTLE_ATR_MULT Live-Path Extensive Sweep

Generated: 2026-05-06 09:10 UTC

## Scope

Closed the stale uncommitted sweep `examples/turtle_atr_mult_live_extensive.rs` instead of leaving it dangling in the working tree.

- Parameter: `TURTLE_ATR_MULT`
- Range: 0.50..=5.00 step 0.05 (91 values)
- Harness: current live-style Turtle-only path with EP=21, ATR period=24, HOLD_MAX=12, CAP=3, ATR_RANK(AP=17, LB=41, T=5), HEDGE_SIZE_MULT=0.40
- Validation rows produced by available 252d windows across configured universes: 60 windows/value

## Result

| Rank | Mult | Pass | Avg Sharpe | Avg Return | Avg MaxDD | Trades | Win rate |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 2.00 | 47/60 (78.3%) | 1.294 | +20.54% | 11.87% | 3,118 | 49.2% |
| 2 | 1.95 | 46/60 (76.7%) | 1.266 | +19.69% | 11.98% | 3,207 | 49.7% |
| 3 | 1.80 | 44/60 (73.3%) | 1.179 | +18.71% | 10.89% | 3,416 | 49.0% |
| 4 | 1.15 | 44/60 (73.3%) | 0.793 | +11.20% | 8.50% | 4,601 | 48.9% |
| 5 | 4.90 | 43/60 (71.7%) | 1.274 | +31.72% | 16.10% | 2,011 | 53.0% |

## Verdict

`TURTLE_ATR_MULT=2.00` remains the robustness winner. No production config change. This closes T74; do not leave the sweep uncommitted or re-run nearby ATR multiplier comparisons without a new mechanism.

## Files

- `examples/turtle_atr_mult_live_extensive.rs`
- `snapshots/turtle_atr_mult_live_summary.csv`
- `snapshots/turtle_atr_mult_live_windows.csv`
- `snapshots/turtle_atr_mult_live_equity.csv`
- `snapshots/turtle_atr_mult_live_summary.json`
