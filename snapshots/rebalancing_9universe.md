# S6 Rebalancing 9-Universe Validation

Candidate discovered on Base5: close_losers interval=5. This validates it across the standard 9-universe guardrail set.

| Universe | Type | Interval | Pass | Avg Sharpe | Avg Ret% | Avg DD% | Trades | WinRate |
|----------|------|----------|------|-----------|----------|---------|--------|---------|
| Base5 | none | 5 | 5/6 | +4.380 | +77.2 | +70.9 | 157 | 55% |
| Base5 | close_losers | 5 | 6/6 | +7.526 | +74.7 | +70.2 | 122 | 68% |
| Base5 | trim_losers | 5 | 6/6 | +4.380 | +64.5 | +69.5 | 157 | 55% |
| NoDOGE | none | 5 | 5/6 | +4.416 | +62.9 | +72.7 | 156 | 54% |
| NoDOGE | close_losers | 5 | 6/6 | +7.517 | +63.0 | +72.2 | 127 | 65% |
| NoDOGE | trim_losers | 5 | 6/6 | +4.416 | +63.1 | +69.4 | 156 | 54% |
| Legacy4 | none | 5 | 6/6 | +4.795 | +100.5 | +71.8 | 166 | 51% |
| Legacy4 | close_losers | 5 | 6/6 | +7.343 | +107.0 | +72.0 | 132 | 63% |
| Legacy4 | trim_losers | 5 | 6/6 | +4.795 | +104.4 | +72.0 | 166 | 51% |
| Legacy5BNB | none | 5 | 6/6 | +5.279 | +172.6 | +72.0 | 168 | 55% |
| Legacy5BNB | close_losers | 5 | 6/6 | +7.344 | +165.0 | +72.6 | 137 | 66% |
| Legacy5BNB | trim_losers | 5 | 6/6 | +5.279 | +125.2 | +72.1 | 168 | 55% |
| OldGuardNoBNB | none | 5 | 5/6 | +3.604 | +80.7 | +72.1 | 165 | 47% |
| OldGuardNoBNB | close_losers | 5 | 5/6 | +7.141 | +83.3 | +72.0 | 126 | 61% |
| OldGuardNoBNB | trim_losers | 5 | 6/6 | +3.604 | +87.0 | +70.4 | 165 | 47% |
| LargeCaps5 | none | 5 | 6/6 | +4.520 | +88.4 | +73.0 | 162 | 55% |
| LargeCaps5 | close_losers | 5 | 6/6 | +7.541 | +89.2 | +73.3 | 130 | 67% |
| LargeCaps5 | trim_losers | 5 | 6/6 | +4.520 | +75.4 | +69.6 | 162 | 55% |
| Legacy3 | none | 5 | 6/6 | +4.086 | +141.7 | +71.0 | 157 | 48% |
| Legacy3 | close_losers | 5 | 6/6 | +7.185 | +150.4 | +71.0 | 120 | 62% |
| Legacy3 | trim_losers | 5 | 6/6 | +4.086 | +134.6 | +70.1 | 157 | 48% |
| LowVolume5 | none | 5 | 3/6 | +1.029 | +32.9 | +70.3 | 159 | 45% |
| LowVolume5 | close_losers | 5 | 3/6 | +4.338 | +38.1 | +70.0 | 120 | 56% |
| LowVolume5 | trim_losers | 5 | 4/6 | +1.029 | +33.9 | +68.4 | 159 | 45% |
| OldGuard4 | none | 5 | 4/6 | +2.338 | +32.9 | +70.4 | 165 | 44% |
| OldGuard4 | close_losers | 5 | 4/6 | +6.117 | +38.9 | +70.5 | 121 | 57% |
| OldGuard4 | trim_losers | 5 | 5/6 | +2.338 | +42.1 | +68.1 | 165 | 44% |

## Global Summary

| Type | Interval | Pass | Avg Sharpe | Avg Ret% | Avg DD% | Trades | WinRate |
|------|----------|------|-----------|----------|---------|--------|---------|
| none | 5 | 46/54 | +3.828 | +87.8 | +71.6 | 1455 | 51% |
| close_losers | 5 | 48/54 | +6.895 | +89.9 | +71.5 | 1135 | 63% |
| trim_losers | 5 | 51/54 | +3.828 | +81.1 | +69.9 | 1455 | 51% |

## Verdict

- **close_losers I=5**: CANDIDATE. Pass rate 48/54 (88.9%, Δ +3.7pp), Sharpe Δ +3.067, trades 1135.

- **trim_losers I=5**: REJECTED. Pass rate 51/54 (94.4%, Δ +9.3pp), Sharpe Δ +0.000, trades 1455.
