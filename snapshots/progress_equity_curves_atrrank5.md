# Progress Equity Curves — ATR_RANK=5 Variant

Generated: 2026-05-04 13:16:11.273425830 UTC

**T56 FEE FIX APPLIED (2026-05-04):** Prior equity figures were OVERSTATED due to fee sign error.
- Prior: `entry = entry_px * (1.0 - TAKER_FEE)` — WRONG
- Fixed: `entry = entry_px * (1.0 + TAKER_FEE)` — correct

Universe: Base5 (BTCUSDT, ETHUSDT, SOLUSDT, XRPUSDT, DOGEUSDT, ADAUSDT)

This file is the SAME turtle run WITH ATR_RANK=5 entry filter applied.
Compare with snapshots/progress_equity_curves.md for the unfiltered baseline.

Final equity | Reported Sharpe:
- Turtle+ATR_RANK=5 (production config): 33.9x (3285.1%), Sharpe 0.42 [daily compounded equity; fee-corrected T56]
- Comparable baseline (no ATR rank): 86.9x (8589.8%), Sharpe 0.43 [daily compounded equity; fee-corrected T56]
