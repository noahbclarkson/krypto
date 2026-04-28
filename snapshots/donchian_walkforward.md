# Donchian Entry Walk-Forward (T19)

**Entry difference:** Donchian = `close > max(high)` (strictest — all-time high breakout). Turtle = `close > max(close)` (breakout above highest close).

**Exit:** Both use Chandelier(7,2.30) + Turtle ATR(24,2.0) dual exit — identical.

| Metric | Donchian | Turtle | Delta |
|--------|----------|--------|-------|
| Pass Rate | 6/7 (86%) | 7/7 (100%) | -14 pp |
| Avg Sharpe | +9.407 | +5.596 | +3.810 |
| Avg Return | +183.8% | +243.0% | -59.2% |
| Total Trades | 76 | 91 | -15 |

**Result: Donchian WINS** — tighter entry produces higher quality signals.

## Per-Window Results

# Donchian Entry Walk-Forward (T19)

| Window | Donchian Pass | Don Sharpe | Don Ret% | Don Trades | Turtle Pass | Tur Sharpe | Tur Ret% | Tur Trades | ΔSharpe | ΔRet |
|--------|---------------|------------|----------|------------|-------------|------------|----------|------------|---------|-----|
| W00 | ✅ | +9.199 | +214.0% | 12 | ✅ | +9.479 | +379.6% | 14 | -0.280 | -165.6% |
| W01 | ✅ | +7.903 | +367.4% | 9 | ✅ | +7.329 | +925.7% | 11 | +0.575 | -558.3% |
| W02 | ✅ | +15.946 | +237.2% | 11 | ✅ | +7.561 | +213.4% | 12 | +8.385 | +23.8% |
| W03 | ✅ | +9.407 | +222.0% | 13 | ✅ | +7.446 | +117.5% | 14 | +1.960 | +104.5% |
| W04 | ✅ | +15.747 | +156.9% | 11 | ✅ | +1.331 | +3.4% | 13 | +14.415 | +153.5% |
| W05 | ❌ | +0.511 | -5.9% | 10 | ✅ | +1.487 | +3.2% | 13 | -0.977 | -9.0% |
| W06 | ✅ | +7.133 | +95.0% | 10 | ✅ | +4.541 | +58.1% | 14 | +2.592 | +36.9% |
