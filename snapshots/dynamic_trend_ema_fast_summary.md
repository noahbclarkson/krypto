# DynamicTrend EMA Fast Period Hyperopt -- 2026-04-16

**Strategy:** DynamicTrend (EMA fast/slow crossover + RSI filter)
**Hyperopt target:** `ema_fast` in [1, 100] step=1 (100 values)
**Fixed params:** ema_slow=100, rsi_filter=50
**Universes:** 4 (Base5, NoDOGE, LargeCaps5, Legacy4)
**Method:** Walk-forward 252-bar train / 252-bar test

**Baseline (ema_fast=50):**
- Pass rate: 67.9% (19/28)
- Avg Sharpe: -13.017
- Avg DD: 0.00%
- Avg Return: 101.89%
- Total Trades: 863

**Winner (ema_fast=60):**
- Pass rate: 85.7% (24/28)
- Avg Sharpe: -25.163
- Avg DD: 0.00%
- Avg Return: 85.03%
- Total Trades: 1107

**Delta vs baseline:** Sharpe -93.3%, DD 0.0pp

**Charts:** `charts/dynamic_trend_comparison.png`, `charts/dynamic_trend_sweep_overview.png`
**Data:** `snapshots/dynamic_trend_ema_fast_global.csv`

**Top 10 EMA Fast periods:**
| ef | Pass% | Sharpe | DD% | Ret% | Trades |
|---|------|--------|-----|------|--------|
| 60 | 85.7 | -25.163 | 0.0 | 85.0 | 1107 |
| 63 | 82.1 | -22.353 | 0.0 | 84.4 | 1065 |
| 59 | 82.1 | -25.254 | 0.0 | 79.4 | 1079 |
| 65 | 82.1 | -27.538 | 0.0 | 83.5 | 1029 |
| 81 | 82.1 | -29.617 | 0.0 | 143.1 | 1075 |
| 79 | 78.6 | -1.499 | 0.0 | 113.1 | 1069 |
| 61 | 78.6 | -29.683 | 0.0 | 103.5 | 1082 |
| 62 | 78.6 | -38.802 | 0.0 | 94.9 | 1084 |
| 85 | 78.6 | -39.632 | 0.0 | 112.9 | 1055 |
| 83 | 75.0 | -8.236 | 0.0 | 129.6 | 1011 |
