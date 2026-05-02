# T40: Regime-Adaptive Exit Walk-Forward Results

Universe: Base5 | Windows: 7 | Train/Test: 252/252

| Config | Pass | Avg Sharpe | Avg Ret% | Avg DD% | Trades | Win% |
|--------|------|------------|---------|---------|--------|------|
| baseline | 6/7 (85.7%%) | 6.425 | 310.6%% | 24.4%% | 91 | 62.5%% |
| rae_1.1_0.9 | 5/7 (71.4%%) | 6.329 | 303.4%% | 24.4%% | 91 | 62.2%% |
| rae_1.2_0.8 | 7/7 (100.0%%) | 6.520 | 254.1%% | 26.5%% | 92 | 58.2%% |
| rae_1.15_0.85 | 5/7 (71.4%%) | 6.289 | 253.3%% | 27.9%% | 91 | 60.2%% |

## Verdict

**Candidate: rae_1.2_0.8** vs baseline (fixed M=2.30)

| Metric | Baseline | rae_1.2_0.8 | Delta |
|--------|----------|------|-------|
| Pass rate | 6/7 (85.7%) | 7/7 (100.0%) | +1 |
| Avg Sharpe | 6.425 | 6.520 | +0.095 |
| Avg Return | 310.6% | 254.1% | -56.5pp |

**MARGINAL — beats baseline by <3 windows. Anti-overfit: NOT promoted.**

