# HEDGE_SIZE_MULT Extensive Sweep — T66

**Mission:** Strip assumptions — HEDGE_SIZE_MULT=0.70 was a hardcoded magic number never independently tested.
**Harness:** Turtle-only + ATR_RANK(AP=17,LB=42,T=5) + USDT hedge (PCT=45 fixed)
**Test:** 13 values × 9 universes × 7 windows = 819 simulations

| HEDGE_SIZE_MULT | Pass | Pass% | Sharpe | Ret% | DD% | Trades | Base5 Equity |
|----------------|------|-------|--------|------|-----|--------|--------------|
| 0.30 | 58/63 | 92.1% | 7.651 | 60.7% | 14.3% | 694 | 32.5875x |
| 0.40 | 59/63 | 93.7% | 7.577 | 72.4% | 16.0% | 694 | 46.9504x |
| 0.50 | 59/63 | 93.7% | 7.432 | 85.3% | 17.7% | 694 | 65.2214x |
| 0.55 | 59/63 | 93.7% | 7.346 | 92.1% | 18.6% | 694 | 75.9330x |
| 0.60 | 58/63 | 92.1% | 7.258 | 99.3% | 19.4% | 694 | 87.7332x |
| 0.65 | 58/63 | 92.1% | 7.168 | 106.8% | 20.3% | 694 | 100.6341x |
| 0.70 | 58/63 | 92.1% | 7.079 | 114.6% | 21.2% | 694 | 114.6325x |
| 0.75 | 58/63 | 92.1% | 6.991 | 122.7% | 22.1% | 694 | 129.7078x |
| 0.80 | 58/63 | 92.1% | 6.906 | 131.2% | 23.0% | 694 | 145.8213x |
| 0.85 | 57/63 | 90.5% | 6.823 | 139.9% | 23.9% | 694 | 162.9143x |
| 0.90 | 57/63 | 90.5% | 6.743 | 149.0% | 24.7% | 694 | 180.9076x |
| 0.95 | 56/63 | 88.9% | 6.666 | 158.5% | 25.7% | 694 | 199.7005x |
| 1.00 | 56/63 | 88.9% | 6.592 | 168.2% | 26.6% | 694 | 219.1708x |

**Robustness winner (pass rate):** SM=0.40 → 59/63 pass (93.7%), Sharpe 7.577
**Sharpe winner:** SM=0.30 → Sharpe 7.651, 58/63 pass

**Baseline comparison:** HEDGE_SIZE_MULT=1.00 (no size reduction) is the reference.
