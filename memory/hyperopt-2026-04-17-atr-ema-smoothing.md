# ATR EMA Smoothing Hyperopt — 2026-04-17

## Parameter Tested: Chandelier ATR EMA Period

**Hypothesis:** ATR values naturally fluctuate day-to-day. Smoothing them with an EMA before using in Chandelier and Turtle ATR trailing stops could reduce "stop-hopping" noise and improve risk-adjusted returns.

**Background:** The current `turtle_chandelier_walkforward.rs` uses RAW ATR values in Chandelier stops. The `live_turtle_chandelier.rs` uses a rolling buffer average (implicitly SMA over CHAND_PERIOD bars). Neither explicitly applies EMA smoothing to the ATR series itself.

**Scope:** ATR_EMA_PERIOD ∈ {1..30} — 30 values × 9 universes × ~6 windows = 1620 simulations.

## Results

| ATR_EMA | Avg Sharpe | Avg Return % | Pass Rate | Trades | Verdict |
|---------|-----------|-------------|-----------|--------|---------|
| **1 (raw)** | 8.4154 | 143.0% | 88.9% | 576 | **BASELINE** |
| **3** | **8.4418** | **143.0%** | **88.9%** | **576** | **★ WINNER** |
| 4 | 8.4214 | 142.8% | 88.9% | 576 | Runner-up |
| 5 | 8.4214 | 142.8% | 88.9% | 576 | Runner-up |
| 2 | 8.4154 | 143.0% | 88.9% | 576 | = baseline |
| 6 | 8.2029 | 141.5% | 88.9% | 579 | Declining |
| 10 | 8.1577 | 140.1% | 88.9% | 587 | Lower |
| 20 | 5.9125 | 107.0% | 87.0% | 623 | Low |
| 30 | 5.7786 | 102.5% | 88.9% | 643 | Lowest |

## Winner: ATR_EMA_PERIOD = 3

- **Avg Sharpe:** 8.4418 vs 8.4154 baseline = **+0.0264 (+0.3%)**
- **Pass rate:** 48/54 (88.9%) — identical to baseline
- **Equity curves:** ATR_EMA=1 and ATR_EMA=3 produce **identical** BTC equity curves (1.331x final equity for both)

## Interpretation

The improvement is **essentially noise**. ATR_EMA=3 barely outperforms raw ATR (EMA=1) and the improvement (+0.3%) is within measurement uncertainty. The BTC equity curves are literally identical — the smoothing has no material effect on the stop position for this single-symbol case.

**Key insight:** ATR naturally changes slowly (it's an average over CHAND_PERIOD=20 bars of range). EMA smoothing of ATR with periods 2-5 provides almost no additional smoothing over raw ATR. The ATR value at bar T already incorporates ~20 bars of smoothing via the true range calculation.

**The trend is clearly negative for large EMA periods.** ATR_EMA=20-30 produces significantly worse Sharpe (5.9-6.0 vs 8.4). Heavy smoothing makes the Chandelier stop lag far behind current ATR, widening the effective stop distance and increasing losses.

## Practical Recommendation

**ATR_EMA_PERIOD = 1 (raw ATR) is the practical default.**
- The code already uses raw ATR — no change needed.
- ATR_EMA=3 provides no meaningful improvement.
- The hypothesis (smoothing reduces stop-hopping) is not supported by the data.
- Do NOT use ATR_EMA > 10 — performance degrades significantly.

## Chart

Generated: `charts/turtle_atr_ema_sweep.png` (2-panel: Sharpe bar chart + equity curves)
Generated: `charts/turtle_atr_ema_sweep_full.png` (full sweep Sharpe plot with pass rate)

## Other Findings

- **This was an UNTESTED parameter** — none of the prior hyperopt sessions tested ATR EMA smoothing
- The sweep ran in 8.9 seconds (30 × 9 × 6 = 1620 simulations)
- All other Turtle+Chandelier parameters remain frozen from prior hyperopt sessions

## Conclusion

**Null result.** ATR EMA smoothing provides negligible benefit. Raw ATR (EMA=1) remains the production default. The ATR calculation already provides sufficient smoothing via the true range averaging mechanism.
