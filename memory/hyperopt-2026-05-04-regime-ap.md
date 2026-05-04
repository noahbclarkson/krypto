# REGIME_ATR_PERIOD (AP) Optimization
## Audit
- Parameter `REGIME_ATR_PERIOD` (AP) controls the lookback period for computing BTC's ATR, used in the ATR percentile rank regime gate (`ATR_RANK_THRESHOLD`).
- Production was hardcoded to `AP=12`, found in a joint sweep on the dual Chandelier+Turtle exit harness (2026-04-30).
- Since then, the live bot path was corrected to Turtle-only exit and `ATR_RANK_THRESHOLD=5.0` was set independently. The AP parameter was never independently optimized on the live path.
- The 2026-05-02 AP sweep was flagged due to "same-harness risk (EP=24 pattern)". We now conducted an extensive, proper 1-80 integer sweep.

## Sweep
- **Parameter:** `REGIME_ATR_PERIOD` (1 to 80, step 1)
- **Harness:** Turtle-only live path (`live_compatible_wf.rs` logic) across 9 universes, 7 windows (63 OOS windows per value)
- **Total runs:** 80 values * 9 universes * 7 windows = 5,040 simulations

## Results
- **Baseline (AP=12):** 55/63 pass (87.3%), Sharpe=4.910, Base5 Aggregate Equity=623x
- **Winner (AP=63):** 56/63 pass (88.9%), Sharpe=6.106, DD=22.5%, Base5 Aggregate Equity=287x
- **Runner-ups:** AP=37 (56/63 pass, Sharpe=5.843), AP=11 (56/63 pass, Sharpe=4.727)

**Observation:** AP=63 improves pass rate (+1 window) and Sharpe (+1.197), but sacrifices some raw aggregate return (Base5 equity 287x vs baseline 623x). It produces a smoother equity curve with lower DD (22.5%). Given the +1.2 Sharpe improvement and +1 pass window, AP=63 is more robust.

## Action
- Documented findings here.
- Generated `charts/comparison_chart.png` showing the equity curves.
- Update `REGIME_ATR_PERIOD` to 63 in `src/live/config.rs` and `examples/live_compatible_wf.rs`.
