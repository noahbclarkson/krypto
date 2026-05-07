# C18: Maker-Fill Rate Stress Test
Generated: 2026-05-07 03:31 UTC

## Context

- **Exact-live baseline:** 2.78x final equity, 28.2% MaxDD, 298 trades.
- **Baseline fee model:** taker entry + taker exit at 4bps/side.
- **Stress question:** if entry maker-fill degrades to 30-50%, does equity fall below deployable range?
- **Decision trigger:** if equity < 1.5x at 40% maker fill → escalate to Arc before C16.

## Correct Fee Math

For a long trade:

```text
entry_exec = raw_entry * (1 + entry_fee)
exit_exec  = raw_exit  * (1 - exit_fee)
net_return = raw_exit/raw_entry * (1 - exit_fee)/(1 + entry_fee) - 1
```

The exact-live trade CSV already contains taker/taker net returns. C18 backs out the raw price ratio, then reapplies entry fee as maker/taker blend. Exit remains taker because Turtle ATR stop exits must be marketable.

## Results

| maker_fill | equity_mult | fee_adj_sharpe* | max_dd | vs taker/taker baseline |
|---|---|---|---|---|
| 30% | 2.801x | 0.839 | 28.1% | +0.8% |
| 35% | 2.804x | 0.840 | 28.1% | +0.9% |
| 40% | 2.808x | 0.841 | 28.1% | +1.0% ← stress threshold |
| 45% | 2.812x | 0.842 | 28.0% | +1.2% |
| 50% | 2.815x | 0.843 | 28.0% | +1.3% |
| 60% | 2.822x | 0.845 | 28.0% | +1.5% |
| 70% | 2.830x | 0.847 | 28.0% | +1.8% ← FTX observed |
| 80% | 2.837x | 0.848 | 27.9% | +2.1% |
| 100% | 2.851x | 0.852 | 27.9% | +2.6% ← all-maker entry |

*Sharpe is computed from the exact-live daily equity curve after applying cumulative per-trade fee-adjustment ratios; baseline differs slightly from the Rust report's internal Sharpe implementation, so use the **relative** change.

## Key Findings

- **At 40% maker fill:** equity **2.81x**, fee-adjusted Sharpe **0.841**, MaxDD **28.1%**.
  → ✅ Above the 1.5x escalation threshold.
- **At 30% maker fill:** equity **2.80x** — still viable.
- **At 70% maker fill (FTX-observed proxy):** equity **2.83x**.
- **Full sensitivity range:** 30%→100% maker fill moves equity only **+1.8%** (2.80x → 2.85x).
- Maker-fill is not the dominant risk. Directional edge, universe sensitivity, and top-winner concentration matter more.

## Decision

✅ **C18: ACCEPTABLE.** Equity remains well above 1.5x at 40% maker fill. No Arc escalation needed. C16 may proceed if it is still mechanistically relevant.
