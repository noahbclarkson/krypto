# Kira | 2026-04-08 | Hyperparameter Optimization: MACD Signal

## The Mission
Audit hardcoded assumptions and magic numbers in the strategies, find a candidate to optimize via extensive parameter sweeping, and validate.

## Step 1: Audit & Selection
I reviewed `src/algo/strategies.rs` and the previous session log `hyperopt-2026-04-08.md`. 
The **MACD Trend** strategy is a core component. In the last session, I optimized the `fast` and `slow` EMA periods (moving them from the 1970s stock market default of 12/26 to a crypto-optimized 14/30). 
However, the **MACD signal period** was left untouched, remaining hardcoded at Gerald Appel's original `9` bars. There was no evidence that a 9-bar smoothing of the MACD line is optimal for the 24/7 crypto market.

**Target Parameter for Optimization:** `signal` EMA period in `MacdTrend`.

## Step 2 & 3: Optimization Execution
I built a dedicated hyperparameter optimization script (`examples/macd_signal_period_hyperopt.rs`) using our strict 9-universe, walk-forward, 15-CPCV resample methodology. 
- `fast`: Fixed at 14 (from prior optimization)
- `slow`: Fixed at 30 (from prior optimization)
- `signal`: Swept exhaustively from **5 to 30** in steps of 1.

*Criteria:* Chronology-first scoring (quarter passes > resample passes > Sharpe) to find the most robust smoothing parameter across all hostile universes, not just the one with the highest in-sample return.

**Findings:**
- **Baseline (9)**: Solid but sub-optimal in hostile universes, producing slightly more false signals during chop.
- **Winner (10)**: A slightly slower signal line (10 bars) improved the pass rate and Sharpe ratio across multiple universes (notably Base5, LargeCaps5, and Legacy4). It provided slightly better noise reduction without introducing too much lag, winning 4 out of the 9 universes and yielding the highest global average quarter pass rate and Sharpe.
- **Runner Up (11 & 12)**: As the signal period increased beyond 10, the lag became detrimental, missing critical momentum shifts during rapid crypto recoveries. Periods 11 and 12 were viable but consistently underperformed 10 in total return and pass count.

*Time-series equity curves were exported to CSV and charted using a custom Python Matplotlib script.*

## Step 4: Stable Default Update
The default parameters for `MacdTrend` in `src/algo/strategies.rs` have been updated to `(fast: 14, slow: 30, signal: 10)`.

## Step 5: Report
A high-quality Matplotlib chart plotting the time-series equity curves (log scale) of the Baseline (9), Winner (10), and Runner-Up (11) has been generated and saved to `/home/ubuntu/.openclaw/workspace-krypto/krypto/charts/comparison_chart.png`.

Sending this chart and summary to Noah in the Discord channel.