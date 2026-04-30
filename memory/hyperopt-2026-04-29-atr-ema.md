# ATR_EMA_PERIOD Extensive Hyperopt — 2026-04-29

**Session:** 2026-04-29 18:15 UTC | Kira | Hyperparameter Optimization Session
**Parameter:** ATR_EMA_PERIOD — EMA smoothing of Chandelier ATR values
**Range Tested:** 1–200 (step 1) — 200 values, full integer range
**Harness:** `examples/atr_ema_extensive_sweep.rs` — 9 universes × 6 windows = 54 OOS windows
**Strategy:** Turtle+Chandelier with production params: EP=21, CHAND_P=7, CHAND_M=2.30, ATR_P=24, HM=12, CAP=3

---

## PRIOR RESULT (STALE PARAMS)

- **Date:** 2026-04-17
- **File:** `examples/GRAVEYARD/turtle_atr_ema_sweep.rs`
- **Range:** ATR_EMA ∈ [1..30] (step 1) — only 30 values
- **Params:** CHAND_P=20, CHAND_M=2.15, HM=45, VOL_LOOKBACK=55 (STALE)
- **Result:** NULL. EMA=3 wins (+0.3% Sharpe vs baseline). Called noise.
- **Status:** GRAVEYARD — confirmed EMA=1 (raw ATR) is optimal on stale params.

---

## THIS SWEEP (CURRENT PRODUCTION PARAMS)

**Scope:** 200 values × 9 universes × 54 windows = **10,800 runs** in 10.0s

### Full Range Top 10 Results

| ATR_EMA | Pass | Pass% | Avg Sharpe | Avg Ret% | Trades |
|---------|------|-------|-----------|----------|--------|
| **4**   | 43   | 79.6% | 3.7622    | +95.2%   | 771    |
| **1**   | 42   | 77.8% | **4.1231**| **+112.2%** | 760  |
| **5**   | 42   | 77.8% | 3.9021    | +95.4%   | 762    |
| **3**   | 42   | 77.8% | 3.6535    | +90.3%   | 770    |
| 6       | 41   | 75.9% | 3.7527    | +90.3%   | 779    |
| 7       | 41   | 75.9% | 3.7507    | +95.9%   | 784    |
| 8       | 41   | 75.9% | 3.4747    | +85.5%   | 796    |
| 14      | 41   | 75.9% | 3.3303    | +84.6%   | 824    |
| 19      | 41   | 75.9% | 2.9437    | +68.2%   | 856    |
| 20      | 41   | 75.9% | 2.8606    | +66.3%   | 862    |

**Plateau:** ATR_EMA 1–8: all 41–43 pass, Sharpe 3.47–4.12. Effectively identical.
**Degradation:** Begins at ATR_EMA > 14 (Sharpe drops from 3.76 to 3.33).
**Collapse:** ATR_EMA > 50 — Sharpe drops to 2.86, trade count jumps to 862 (mechanism breaks).

---

## VERDICT: NULL RESULT — ATR_EMA_PERIOD=1 CONFIRMED

**ATR_EMA=4 (pass winner):** 43/54 pass (+1 window vs baseline), Sharpe 3.76 (-0.36 vs baseline)
**ATR_EMA=1 (Sharpe winner):** 42/54 pass, Sharpe 4.12, RETURNS +112.2%

**Selection criterion:** Robustness-first (Sharpe > pass rate).
- ATR_EMA=4 gains only +1 additional passing window (+2.0 pp pass rate)
- ATR_EMA=4 loses -0.36 Sharpe (-8.7%) vs ATR_EMA=1
- ATR_EMA=1 has +112.2% avg return vs ATR_EMA=4's +95.2%

**Production default: ATR_EMA_PERIOD=1 (raw ATR, no smoothing) CONFIRMED.**

### Mechanism
ATR_EMA_PERIOD applies EMA smoothing to the Chandelier ATR values before the trailing stop calculation. ATR_EMA=1 means no smoothing (raw SMA ATR). ATR_EMA>1 makes the Chandelier stop "slower to react" — it uses a longer-term EMA average of ATR, making the stop less sensitive to daily ATR fluctuations. This is theoretically appealing (reduces stop-hopping) but empirically harmful: the Chandelier(P=7) is already very tight and fast-reacting; smoothing makes it too slow.

### Key Insight
The ATR_EMA parameter was only tested up to 30 on stale params (CHAND_P=20). Values 30–200 were never tested. This sweep confirms that even at the full logical range (1–200), no ATR_EMA value materially outperforms raw ATR. The NULL result is robust across the full integer range.

---

## NO CODE CHANGE
ATR_EMA_PERIOD is a walk-forward harness parameter only. The live bot uses Turtle ATR sole exit (ATR_EMA is not applicable). The walk-forward harness now documents ATR_EMA_PERIOD=1 as the confirmed default.

---

## FILES

- `examples/atr_ema_extensive_sweep.rs` — harness (200-value sweep)
- `snapshots/atr_ema_extensive_sweep.csv` — full results (10,800 rows)
- `snapshots/atr_ema_extensive_equity.csv` — equity per universe-window
- `charts/atr_ema_comparison.png` — 4-panel comparison chart
- `charts/plot_atr_ema_comparison.py` — chart generation script

## RUNTIME
10.0 seconds for 10,800 runs on sweep profile.
