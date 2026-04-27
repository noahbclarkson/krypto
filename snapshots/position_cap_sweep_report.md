# POSITION_CAP Hyperopt — 2026-04-27

- Strategy: current Turtle-only production logic
- Fixed params: EP=21, ATR(24, 2), ATR_ENTRY_MULT=0, HM=12, VOL_LOOKBACK=9
- Sweep: POSITION_CAP ∈ [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
- Selection rule: pass rate > positive universes > avg Sharpe > lower avg max DD

## Winner
- Winner: CAP=3 | pass=72.2% | avg Sharpe=4.577 | avg return=148.9% | avg DD=32.9% | positive universes=9/9
- Baseline CAP=3 | pass=72.2% | avg Sharpe=4.577 | avg return=148.9% | avg DD=32.9%

## Selected equity exports
- snapshots/position_cap_all_equity.csv
- snapshots/position_cap_selected_equity.csv
- snapshots/position_cap_sweep_summary.csv
- snapshots/position_cap_sweep_detail.csv
- snapshots/position_cap_sweep_summary.json
