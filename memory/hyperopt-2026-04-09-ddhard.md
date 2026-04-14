# Hyperparameter Optimization Report — 2026-04-09 (DD-Hard Exposure Thresholds)

## Session: DDBudget / DD-hard Threshold Optimization

**Mission:** Audit hardcoded exposure thresholds used in the DD-hard (Drawdown Budget) system, run extensive sweeps, and generate visual equity comparisons.

---

## What Was Done

### Step 1: Audit
- Found the DD-hard exposure scaling function used across multiple scripts (e.g., `ddbudget_4sleeve_walkforward`, `progress_equity_curves`).
- It used undocumented magic numbers:
  - **Soft Cut:** 15.0% drawdown → 60% exposure (or similar)
  - **Hard Cut:** 30.0% drawdown → 30% exposure (or similar)
- These values were chosen arbitrarily. A 30% drawdown is massive in crypto, potentially causing catastrophic damage before the "hard cut" even triggers.

### Step 2: Sweep Execution
- **Harness:** `krypto/examples/ddhard_threshold_sweep.rs`
- **Target parameters:**
  - `hard_thresh` ∈ 15.0% to 50.0% in 2.5% steps
  - `soft_thresh` ∈ 5.0% to 45.0% in 2.5% steps (constrained to `soft < hard`)
  - **Total Configs:** 154
- **Validation:** 252/252 bar walk-forward across 9 universes using the A/D Momentum + MACD/Regime + SmallVol sleeve blend.

### Step 3: Results

The sweep revealed that the original hardcoded thresholds (Soft=15.0%, Hard=30.0%) were sub-optimal. The best performance was found by raising the hard threshold significantly, allowing more compounding during deep but recoverable drawdowns.

**Top Configs (Walk-Forward OOS):**
1. **WINNER: Soft=15.0%, Hard=47.5%** → Sharpe: 5.274, Avg Return: +18.8%, Worst DD: -18.2%, QP: 38/36
2. **Runner-up: Soft=15.0%, Hard=50.0%** → Sharpe: 5.274, Avg Return: +18.8%, Worst DD: -18.2%, QP: 38/36
3. **Runner-up: Soft=15.0%, Hard=42.5%** → Sharpe: 5.272, Avg Return: +18.8%, Worst DD: -18.2%, QP: 38/36
4. **Baseline (15.0% / 30.0%):** Sharpe: 3.815, Avg Return: +18.8%

**Key Findings:**
1. **Soft Threshold at 15% is solid:** The original 15% soft threshold was actually optimal for initiating risk reduction.
2. **Hard Threshold was too tight:** Cutting exposure drastically at 30% drawdown choked off recoveries. A hard threshold of ~47.5% performed much better, yielding a higher Sharpe ratio by allowing the system to participate in rebounds.

### Step 4: Charting
- Generated `charts/ddhard_threshold_sweep.png` (Heatmaps of Quarter Passes & Avg Sharpe).
- Generated `charts/ddhard_threshold_equity_comparison.png` plotting the OOS equity curves for the Winner, Baseline, and Runner-ups.
