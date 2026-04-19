# Examples Graveyard — Stale/Unused Files

Archived on 2026-04-19. These files are superseded or invalid.

## Why Archived

- Strategy was rejected in walk-forward validation
- Results were artifacts (look-ahead bias, overfitting, or wrong engine)
- Single-use prototype that served its purpose
- Approach fundamentally flawed

## Strategy Graveyard

| File | Strategy | Reason | Date |
|------|----------|--------|------|
| `bollinger_1d_sweep.rs` | Bollinger Reversion | REJECTED — 0/288 OOS pass. Signal actively harmful. | 2026-04-11 |
| `bollinger_bb_period_hyperopt.rs` | Bollinger BB period | Wasted hyperopt on dead strategy. | 2026-04-11 |
| `bollinger_rsi_filter_hyperopt.rs` | Bollinger RSI filter | Same — curve-fitting on a kill. | 2026-04-11 |
| `ad_period_legacy3.rs` | A/D period legacy | A/D rejected as portfolio sleeve (52% pass). | 2026-04-12 |
| `ddbudget_3sleeve_walkforward.rs` | DDBudget 3-sleeve | Rejected — combined worse than Turtle alone. | 2026-04-16 |
| `dynamic_trend_chandelier_walkforward.rs` | DynamicTrend+Chandelier | REJECTED — Turtle wins 21/24 windows. | 2026-04-16 |
| `intraday_mean_reversion_4h_walkforward.rs` | 4h Mean Reversion | DEAD — 0/4 pass, execution costs destroy edge. | 2026-04-11 |
| `fdusd_basis_carry_benchmark.rs` | FDUSD Basis Carry | DEAD — 19% pass, basis is structural. | 2026-04-10 |
| `btc_eth_cointegration_pair_benchmark.rs` | BTC-ETH Cointegration | DEAD — pair trading fails in crypto. | 2026-04-11 |
| `vol_contingent_chandelier.rs` | Vol-Contingent Chandelier | DEAD — vol_rank too slow, all configs identical. | 2026-04-12 |
| `random_entry_test.rs` | Random Entry Test | Diagnostic only — confirms Turtle signal is real vs random. | 2026-04-10 |
| `ctrend_monte_carlo.rs` | CTREND Monte Carlo | Validated signal GENUINE, but fixed hold makes it a graveyard entry. | 2026-04-16 |

## Parameter / Hyperopt Artifacts (Superseded)

| File | Content | Reason |
|------|---------|--------|
| `turtle_atr_period_full_sweep.rs` | ATR period 5-100 | Superseded by fine sweep ATR=24 |
| `turtle_chand_mult_hyperopt.rs` | Chandelier mult sweep | Superseded by 2D P×M sweep |
| `chandelier_mult_hyperopt.rs` | Chandelier mult sweep | Same |
| `turtle_atr_ema_sweep.rs` | ATR EMA smoothing | NULL result — raw ATR optimal |
| `turtle_freshness_sweep.rs` | Freshness cooldown sweep | Result: cd=0, superseded by comprehensive sweep |
| `turtle_entry_filter_sweep.rs` | ATR entry × vol confirm | REJECTED — both ideas null |
| `entry_ranking_audit.rs` | Entry ranking analysis | One-off diagnostic, not production |
| `parameter_stability.rs` | Parameter stability analysis | One-off diagnostic |

## Legacy / Single-Use Diagnostic Files

| File | Purpose |
|------|---------|
| `backtest_audit.rs` | Audit of backtest engine vs paper bot |
| `benchmark_drift_audit.rs` | Drift detection across backtest versions |
| `change_point_state_overlay.rs` | BOCPD change point detector (BROKEN) |
| `debug_momentum.rs` | Momentum signal debugging |
| `hmm_regime_prototype.rs` | HMM regime detector prototype |
| `signal_index_debug2.rs` | Signal indexing debug |
| `signal_indexing_debug.rs` | Signal indexing debug |
| `stop_loss_test.rs` | Stop loss mechanics testing |
| `time_based_exit.rs` | Time-based exit mechanics |
| `trade_analysis.rs` | Trade-level analysis |
| `portfolio_paper_bot.rs` | Portfolio paper bot prototype |
| `live_bot_demo.rs` | Demo of live bot (superseded by live_turtle_chandelier) |
| `live_bot_1m_test.rs` | 1m live bot test |

## How to Restore

If any archived file is needed:
```bash
git log --all --oneline -- examples/GRAVEYARD/<filename>
git show <commit> -- examples/GRAVEYARD/<filename> > examples/<filename>
```
