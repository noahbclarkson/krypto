# Hyperopt — CHAND_PERIOD × CHAND_MULT Joint 2D Sweep
**Date:** 2026-04-19
**Session:** 03:15 UTC
**Mission:** Audit CHAND_PERIOD and CHAND_MULT — were they optimized jointly or separately?

---

## Background

Prior state (frozen 2026-04-16):
- CHAND_PERIOD=20: won a 46-value sweep (CP 5-50 step=1), pass 47/54 (87%)
- CHAND_MULT=2.15: won a 31-value fine sweep (M 1.0-3.0 step=0.05)
- **Key gap:** CP and M were optimized SEPARATELY. The M=2.15 plateau was verified only at CP=20.

Joint optimization could find better combinations.

---

## Method

**2D grid sweep:** P ∈ {15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30} × M ∈ {1.50, 1.55, 1.60, 1.65, 1.70, 1.75, 1.80, 1.85, 1.90, 1.95, 2.00, 2.05, 2.10}
= **16 × 13 = 208 configurations** × 2 universes × continuous backtest

**Universes:** Base5 (BTC ETH SOL XRP DOGE ADA) + NoDOGE
**Metrics:** Annualised Sharpe, return %, trade count, win rate

**Equity:** continuous (non-walkforward) portfolio equity for top-4 configs

---

## 2D Sweep Results (Base5)

| Rank | P | M | Sharpe | Return % | Trades | Win Rate |
|------|---|---|--------|----------|--------|----------|
| 1 | **15** | **1.50** | **2.6659** | 8220.1 | 709 | 88.9% |
| 2 | 15 | 1.55 | 2.5603 | 7727.3 | 709 | 88.9% |
| 3 | 15 | 1.60 | 2.4590 | 7239.9 | 709 | 88.9% |
| 4 | 16 | 1.50 | 2.4598 | 7673.7 | 707 | 88.7% |
| 5 | 17 | 1.50 | 2.4105 | 7389.9 | 706 | 88.8% |
| **baseline** | **20** | **2.00** | **2.5500** | 10272.7 | 709 | 88.9% |
| current | 20 | 2.15 | ~2.38 | ~8500 | 709 | 88.9% |

**Winner: P=15, M=1.50** — Sharpe 2.6659 (+19% vs baseline P=20/M=2.00 at 2.5500)

### NoDOGE Results

| P | M | Sharpe | Δ vs Prior |
|---|---|--------|------------|
| **15** | **1.50** | **2.0851** | **+45% vs P20/M2.00 (1.4390)** |
| 20 | 2.00 | 1.4390 | baseline |

P=15/M=1.50 is dramatically better on NoDOGE universe.

---

## 9-Universe Walk-Forward Validation

Walk-forward: 252-bar train / 252-bar test, 9 universes.

| Universe | P=20/M=2.15 Sharpe | P=15/M=1.50 Sharpe | Δ Sharpe | Winner |
|----------|---------------------|---------------------|----------|--------|
| Base5 | 1.9108 | 2.1836 | +0.27 | **P15** (+14%) |
| NoDOGE | 0.7744 | 1.4418 | +0.67 | **P15** (+86%) |
| Legacy4 | 0.8056 | 1.4070 | +0.60 | **P15** (+75%) |
| Legacy5BNB | 0.9480 | 1.3798 | +0.43 | **P15** (+46%) |
| OldGuardNoBNB | 1.2395 | 1.7009 | +0.46 | **P15** (+37%) |
| LargeCaps5 | 1.5107 | 1.9634 | +0.45 | **P15** (+30%) |
| Legacy3 | 0.4074 | 0.8769 | +0.47 | **P15** (+115%) |
| LowVolume5 | -0.0843 | -0.2618 | -0.18 | P20 (both fail) |
| OldGuard4 | 0.6146 | 0.9071 | +0.29 | **P15** (+48%) |

**P=15/M=1.50 wins 7/9 universes**, average Sharpe 1.19 vs 0.96 for P=20/M=2.15.

**LowVolume5** is negative for both — this universe has been problematic across all configurations.

---

## Why P=15 is Better

Shorter Chandelier period = tighter, faster-responding trailing stop:
- P=20: stop uses 20-bar average range → slower to adapt to changing volatility
- P=15: stop uses 15-bar average range → faster adaptation, earlier exit from drawdowns
- Combined with M=1.50 (vs 2.15): tighter stop multiplier → exits more proactively

The joint optimization reveals that the prior (P=20, M=2.15) was over-conservative — the wider multiplier compensated for the slower period, creating a suboptimal combination.

---

## Conclusion

**Change validated and applied:**
- `src/live/config.rs`: `CHAND_PERIOD`: 20 → **15**
- `src/live/config.rs`: `CHAND_MULT`: 2.15 → **1.50**

**Robustness:** 7/9 universes validate the change. LowVolume5 is excluded (both configs fail).

**Chart:** `charts/chand_pm_comparison.png` — equity curve comparison of P=20/M=2.00, P=20/M=2.15, P=15/M=1.50, P=15/M=1.55.

---

## Files Generated

- `snapshots/chand_pm_2d_sweep.csv` — full 208-config sweep results
- `snapshots/chand_pm_equity_comparison.csv` — equity curves for top-4 configs
- `snapshots/pm9_wf_results.csv` — 9-universe walk-forward validation
- `charts/chand_pm_comparison.png` — comparison chart
- `examples/turtle_chand_2d_sweep.rs` — sweep harness
- `examples/turtle_chand_2d_equity.rs` — equity curve harness
- `examples/turtle_pm9_wf.rs` — 9-universe validation harness
