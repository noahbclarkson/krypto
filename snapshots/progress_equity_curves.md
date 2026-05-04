# Progress Equity Curves

Generated: 2026-05-04 13:16:11.273280574 UTC

**T56 FEE FIX APPLIED (2026-05-04):** Prior equity figures were OVERSTATED due to fee sign error in `progress_equity_curves.rs` line 875.
- Prior: `entry = entry_px * (1.0 - TAKER_FEE)` — WRONG for longs (made entry cheaper)
- Fixed: `entry = entry_px * (1.0 + TAKER_FEE)` — correct (pay fee on entry)
- Impact: Turtle+Chandelier 113.6x → **86.9x** (−23.5%). ATR_RANK=5 43.1x → **33.9x** (−21.3%)

Universe: Base5 (BTCUSDT, ETHUSDT, SOLUSDT, XRPUSDT, DOGEUSDT, ADAUSDT)

Final equity | Reported Sharpe:
- A/D Momentum: 39.3x (3829.4%), Sharpe 3.47 [fixed-hold daily equity]
- FactorSmallByDV: 15.5x (1445.7%), Sharpe 0.77 [fixed-hold daily equity]
- DDBudget 3-Sleeve: 61.6x (6063.0%), Sharpe 5.45 [milestone-aggregated; not comparable to Turtle daily equity]
- Turtle+Chandelier: 86.9x (8589.8%), Sharpe 0.43 [daily compounded equity; fee-corrected T56]
- Turtle+ATR_RANK=5: 33.9x (3285.1%), Sharpe 0.42 [regime filter T=5.0; daily compounded equity; fee-corrected T56]
[MACD+Regime & Blend excluded: 2/7 OOS pass — GRAVEYARD]
