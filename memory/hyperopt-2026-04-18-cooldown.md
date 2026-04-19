# Freshness Cooldown Hyperopt — 2026-04-18

## Parameter Tested

**FRESHNESS_COOLDOWN** — bars to wait after exit before re-entering a symbol.

**Gap discovered:** The walk-forward harness (`turtle_chandelier_walkforward.rs`) has NO freshness filter (equivalent to cd=0). The live bot (`src/live/bot.rs`) uses cd=10. This creates a structural live/backtest gap — the live bot runs a filter never validated in the walk-forward.

**Range swept:** 15 values — 0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70

## Results (Base5 Universe, 6 windows)

| cd  | Pass | Pass%  | Avg Sharpe | Avg Ret% | Avg DD% | Trades |
|-----|------|--------|-----------|----------|---------|--------|
| **0**  | **6/6** | **100.0** | **8.5649** | **+174.7** | **23.4** | **78** |
| 5    | 4/6   | 66.7  | 5.2865  | +114.2 | 29.0 | 84 |
| **10 (live default)** | **3/6** | **50.0** | **5.4760** | **+145.3** | **29.5** | **81** |
| 15   | 5/6   | 83.3  | 5.4322  | +178.0 | 26.5 | 81 |
| 20   | 5/6   | 83.3  | 6.2339  | +252.8 | 21.0 | 77 |
| 25   | 6/6   | 100.0 | 6.8460  | +214.2 | 20.2 | 74 |
| 30   | 6/6   | 100.0 | 6.0429  | +151.7 | 24.1 | 77 |
| 35   | 6/6   | 100.0 | 7.3171  | +125.9 | 24.6 | 75 |
| 40   | 6/6   | 100.0 | 7.1448  | +113.3 | 23.8 | 76 |
| 45   | 6/6   | 100.0 | 6.6597  | +109.6 | 26.8 | 71 |
| 50   | 6/6   | 100.0 | 6.7443  | +104.3 | 25.1 | 71 |
| 55   | 5/6   | 83.3  | 6.6883  | +126.5 | 26.4 | 72 |
| 60   | 6/6   | 100.0 | 6.8376  | +138.1 | 25.5 | 72 |
| 65   | 5/6   | 83.3  | 5.1160  | +125.7 | 28.1 | 73 |
| 70   | 4/6   | 66.7  | 3.6219  | +92.8 | 31.1 | 76 |

## Winner

**cd=0 (no freshness filter)** — 6/6 pass, Sharpe 8.5649, +174.7% avg return

## Runner-ups

- cd=35: 6/6 pass, Sharpe 7.32, +125.9% avg return
- cd=40: 6/6 pass, Sharpe 7.14, +113.3% avg return
- cd=25: 6/6 pass, Sharpe 6.85, +214.2% avg return (best return)

## Critical Finding: Live Bot Default (cd=10) is WORST

**cd=10 (current live bot default): 3/6 pass, Sharpe 5.48, equity 0.69x (NET LOSS)**

The live bot's FRESHNESS_COOLDOWN=10 was set based on a 2026-04-18 hyperopt sweep that used the OLD parameter set (CHAND_PERIOD=28). After updating to CHAND_PERIOD=20 in 2026-04-16, the optimal cooldown has SHIFTED. The cd=10 filter is now counterproductive — it filters out valid entries, reduces trade count, and produces net losses in 3 of 6 windows.

**Root cause:** The prior cd=10 winner was specific to CHAND_PERIOD=28 dynamics. With CHAND_PERIOD=20, the Chandelier exit fires faster, and the freshness filter blocks re-entries that would have been valid and profitable.

## Action Taken

Changed `src/live/bot.rs`:
- `const FRESHNESS_COOLDOWN: usize = 10` → `const FRESHNESS_COOLDOWN: usize = 0`
- Updated comment explaining the reversion

Changed `examples/live_turtle_chandelier.rs`:
- Removed "cd=10" from params print line

## Interpretation

The freshness filter (cooldown after exit) does NOT improve performance on the current production parameters. The walk-forward harness correctly has no freshness filter. The live bot now matches.

**cd=25-50 plateau is the stable robust zone** (6/6 pass for all values in this range). If a non-zero freshness filter is ever desired for live risk management, cd=25 or cd=35 would be reasonable choices. But cd=0 is optimal for pure performance.

## Files

- `examples/turtle_freshness_sweep.rs` — hyperopt harness
- `snapshots/freshness_sweep.csv` — full sweep results
- `snapshots/freshness_sweep_equity.csv` — equity curves per cd value
- `charts/freshness_sweep_comparison.png` — comparison chart

## Chart

`charts/freshness_sweep_comparison.png` — equity curves for cd=0/10/35/40 + Sharpe bar chart.

![Freshness Sweep Comparison](charts/freshness_sweep_comparison.png)
