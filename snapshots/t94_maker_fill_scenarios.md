# T94: Maker-Fill Scenario Analysis

Generated: 2026-05-10 21:14 UTC

## Dominant Deployment Risk: Maker-Fill Rate

The live bot's daily account Sharpe is estimated at **1.02** under the conservative taker-fee assumption (0.04% both sides). The actual Sharpe depends on the maker-fill rate:

- Turtle entry fires at bar close. In trending markets, limit orders placed at close are filled as **maker**.

- In choppy markets, the limit misses and is filled as taker the next bar.

- Exit stops always trigger **market sells** → always taker.

- Microstructure analyzer (T77): BTC 70.2%, ETH 68.3%, SOL 73.5% maker fill rate.


## Scenario Table

| Scenario | Maker % | Entry Fee (bps) | Equity | Sharpe | MaxDD | Ann. Ret | Trades |
|---|---|---|---|---|---|---|---|
| S1: Pure taker (0% maker — baseline) | 0% | 4.0 | 2.757x | 1.022 | 22.3% | 22.9% | 286 |
| S2: 50% maker fill | 50% | 2.0 | 2.786x | 1.031 | 22.1% | 23.1% | 286 |
| S3: 70% maker fill (microstructure confirmed) | 70% | 1.2 | 2.798x | 1.035 | 22.0% | 23.2% | 286 |
| S4: 80% maker fill (optimistic) | 80% | 0.8 | 2.804x | 1.036 | 21.9% | 23.3% | 286 |

## Interpretation

Baseline (0% maker): 2.757x / Sharpe 1.022
At 70% maker:       2.798x / Sharpe 1.035
Improvement:       Sharpe +0.012 (1.2%)

At 80% maker:       2.804x / Sharpe 1.036
Improvement:       Sharpe +0.014 (1.4%)

**Fee-adjusted Sharpe range: [1.02 – 1.04]** (at 0%–80% maker fill)

Previous estimate was [0.6–1.3]. Confirmed range is tighter: [1.02–1.04].
