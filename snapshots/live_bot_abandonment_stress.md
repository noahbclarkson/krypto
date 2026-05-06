# T68: Exact Live-Bot Abandonment / Risk Governance Stress

Source: `snapshots/live_bot_exact_equity.csv` + `snapshots/live_bot_exact_trades.csv` from the exact T65/T67 live-bot replay. This is a risk-governance audit, not a new strategy comparison.

## Baseline

- Days: 1795
- Final equity: 2.55x
- MaxDD: 28.8%
- Trades: 298

## Drawdown Rules

| Threshold | Breached? | Breach Date | Recovery Wait | Abandon Final | Halt-while-DD Final | Half-risk-in-DD Final | Quarter-risk-in-DD Final | Missed Top-10 Winners |
|---:|---|---|---:|---:|---:|---:|---:|---:|
| 20% | yes | 2022-09-13 | 451d | 1.27x | 2.89x | 2.72x | 2.81x | 3 / 1.42x |
| 30% | no | — | — | 2.55x | 2.55x | 2.55x | 2.55x | 0 / 1.00x |
| 40% | no | — | — | 2.55x | 2.55x | 2.55x | 2.55x | 0 / 1.00x |
| 50% | no | — | — | 2.55x | 2.55x | 2.55x | 2.55x | 0 / 1.00x |
| 70% | no | — | — | 2.55x | 2.55x | 2.55x | 2.55x | 0 / 1.00x |
| 85% | no | — | — | 2.55x | 2.55x | 2.55x | 2.55x | 0 / 1.00x |

## Interpretation

- The exact live-bot curve only breaches the 20% drawdown rule; its measured MaxDD is below 30%.
- A hard 20% abandonment rule would have stopped the bot before later recovery and left final equity near the breach level.
- 30%+ hard stops are not exercised on this sample; they behave like baseline but provide little ex-ante governance beyond the observed MaxDD.
- Best operational default from this audit: monitor 20% DD as a human review trigger, but do not auto-abandon below 30% without live/testnet evidence.
- T68 does not remove the live/testnet blocker; it only clarifies abandonment policy for the current exact-live leader.
