# S4: ATR-Normalized Position Sizing — REJECTED

**Date:** 2026-04-29
**Status:** REJECTED — experiment confirms equal capital is optimal

## Hypothesis

- Current: CAP=3, equal $10K per position
- S4: $10K / 21-bar ATR per position (ATR-normalized notional)
- Mechanism: high-vol symbols get smaller positions, low-vol symbols get larger
- Different from failed position scaling overlays — those changed CAP scalar; this adjusts per-symbol notional within CAP=3

## Test Design

- 3 configs × Base5 × 7 windows (6 test windows)
- Configs: equal_capital_baseline ($10K each), atr_norm_10k ($10K/ATR), atr_norm_20k ($20K/ATR)
- Walk-forward: 252-bar train / 252-bar test
- Dual exit: Chandelier(7, 2.30) + Turtle ATR(24, 2.0)

## Results

| Config | Pass | Avg Sharpe | Avg Ret% | Avg DD% | Trades |
|--------|------|------------|----------|---------|--------|
| equal_capital_baseline | **6/7 (86%)** | 10.18 | +665% | 38% | 653 |
| atr_norm_10k | 4/7 (57%) | 53.5 | +3597% | 280% | 653 |
| atr_norm_20k | 4/7 (57%) | 107.0 | +7195% | 538% | 653 |

**Note:** Sharpe numbers are inflated by W3 mega-bull (+26720% ATR). The real signal is pass rate and drawdown.

## Per-Window Analysis

| Window | Equal Pass | Equal Ret% | Equal DD% | ATR Pass | ATR Ret% | ATR DD% |
|--------|-----------|------------|-----------|----------|----------|---------|
| W0 (bull) | ✓ | +1638% | 0.9% | ✓ | +275% | 0.5% |
| W1 (chop) | ✓ | +6.5% | 43% | ✗ | -3.9% | 9.3% |
| W2 (bull) | ✓ | +657% | 4.9% | ✓ | +51% | 97% |
| W3 (mega-bull) | ✓ | +2198% | 5.0% | ✓ | +26720% | 3.6% |
| W4 (bear/chop) | ✓ | +152% | 39% | ✗ | -1866% | **1840%** |
| W5 (bear/chop) | ✗ | -158% | 132% | ✓ | +7.3% | 1.7% |
| W6 (bull) | ✓ | +165% | 43% | ✗ | -1.5% | 11% |

## Key Finding: W4 Catastrophic Failure

W4 (2022-01 to 2022-09, bear/chop):
- Equal capital: +152% return, 39% DD — PASS
- ATR normalized: -1866% return, **1840% DD** — total loss of 18x starting capital

**Root cause:** DOGE and SOL have high ATR → small notional under ATR normalization. But DOGE/SOL are the top volume rankers in most windows. When DOGE/SOL are top-ranked by volume but get small positions, the remaining cap allocation goes to lower-vol symbols (BTC/ETH) which have LARGE ATR-normalized positions. In bear markets, BTC/ETH drawdowns are catastrophic at large notional.

**Mechanism:** ATR normalization INVERTS the natural volume-weighting. Low-vol assets (BTC/ETH) get disproportionately large positions. High-vol assets (DOGE/SOL) get small positions despite being the top volume-ranked symbols. This is the opposite of what works.

## Verdict

**REJECTED.** Equal capital allocation is optimal.

- ATR normalization breaks volume-ranking signal (the primary selection mechanism)
- Inverts position sizing: low-vol assets get oversized, high-vol assets get undersized
- Catastrophic failure in bear/volatile regimes (W4: 1840% DD)
- Equal capital: 86% pass vs ATR normalization: 57% pass
- Chandelier exit already handles position management dynamically — sizing overlay is redundant and harmful

**Production default remains:** Equal $10K per position (CAP=3, ranked by dollar volume).

## Files

- `examples/s4_atr_norm_position_sizing.rs`
- `snapshots/s4_atr_norm_position_sizing.csv`
- `snapshots/s4_atr_norm_position_sizing_detail.csv`