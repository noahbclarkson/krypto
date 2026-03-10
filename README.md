# Krypto

Config-driven crypto backtesting and quantitative research framework.

## Overview

Krypto is a Rust-based backtesting framework designed for:
- **Reproducible experiments** via config-driven runs
- **Walk-forward validation** to prevent overfitting
- **Realistic cost modeling** (fees + slippage)
- **Auditability** via run manifests

## Quick Start

```bash
# Run an experiment
krypto run configs/example.json

# List all experiment runs
krypto list ./experiments

# Generate example config
krypto example my_config.json
```

## Project Structure

```
krypto/
├── src/
│   ├── bin/krypto.rs       # CLI entrypoint
│   ├── config/             # Experiment configuration schema
│   ├── experiment/         # Run manifest and orchestrator
│   ├── algo/               # Strategy implementations
│   ├── backtest/           # Backtest engine
│   ├── data/               # Data loading
│   └── features/           # Technical indicators
├── configs/                # Example configurations
├── docs/                   # Documentation
├── experiments/            # Experiment outputs (created at runtime)
├── GRAVEYARD.md            # Failed strategies log
└── HALL_OF_FAME.md         # Successful strategies log
```

## Experiment Configuration

Experiments are defined in JSON files. See `configs/example.json` for a full example.

Key sections:
- **data**: Market data source, symbols, timeframe
- **costs**: Trading fees and slippage
- **sizing**: Position sizing rules
- **validation**: Train/test split method
- **metrics**: Evaluation criteria
- **strategy**: Strategy type and parameters

## Validation Methods

| Method | Description | Use Case |
|--------|-------------|----------|
| `simple_split` | Single train/test split | Quick prototyping |
| `walk_forward` | Expanding window | Time series validation |
| `cpcv` | Combinatorial purged CV | Regime-robust testing |

## Documentation

- [Design Tradeoffs](docs/TRADEOFFS.md) - Decision rationale
- [Strategy Graveyard](GRAVEYARD.md) - Failed strategies
- [Hall of Fame](HALL_OF_FAME.md) - Successful strategies

## Requirements

- Rust 1.82+ (required for modern crate ecosystem)
- Cargo

## Passive Execution (0% Maker Fee)

For FDUSD pairs with 0% maker fee, use `Backtester::run_with_passive()` which simulates
placing limit orders below each bar open instead of executing at market.

```rust
use krypto::backtest::passive::{lower_interval_for_signal, bars_per_signal, PassiveConfig, TickSize};

// Auto-select lower timeframe: 1h→5m, 4h→15m, 1d→30m
let lower_iv = lower_interval_for_signal("1h");  // "5m"
let max_wait = bars_per_signal("1h");             // 12

let passive_cfg = PassiveConfig {
    ticks_below_open: 3,
    tick_size: TickSize::fetch("BTCFDUSD").await?,
    max_wait_bars: max_wait,
    maker_fee: 0.0,   // FDUSD 0% maker
    update_threshold_ticks: None,
    anchor_to_signal: true,
};

let (result, fill_stats) = backtester
    .run_with_passive(&df_high, &df_low, &signals, stop, 0.0, passive_cfg)
    .await?;

println!("Fill rate: {:.1}%", fill_stats.fill_rate * 100.0);
println!("Price improvement: {:.1} ticks", fill_stats.avg_price_improvement_ticks);
```

See `examples/fdusd_validation.rs` for a full USDT taker vs FDUSD passive comparison.

## Normalised Metrics

Every `BacktestResult` now includes time-normalised metrics for fair cross-timeframe comparison:

| Field | Description |
|-------|-------------|
| `annualised_return_pct` | Compound annualised return (use instead of total return) |
| `annualised_sharpe` | Sharpe × √(trades/year) — primary ranking metric |
| `trades_per_year` | Trade frequency — proxy for statistical reliability |
| `return_per_trade_pct` | Edge per opportunity |
| `backtest_years` | Transparency on data coverage |

Use `annualised_sharpe` as the primary ranking metric when comparing strategies across
different timeframes (1d covers 7-8y; 1h covers ~1.4y — total return is not comparable).

## ATR-Based Stops

The strategy sweep (`examples/strategy_sweep.rs`) uses ATR multipliers instead of fixed %:

```
stop = ATR_mult × median(ATR(14) / close)
```

Key findings (10 pairs, Bollinger Reversion + Volatility Squeeze):
- **0.5× ATR** dominates: +30.9% avg annualised return
- **≥1× ATR** loses money: -20% to -30% avg annualised return
- 1d timeframe vastly outperforms 4h on annualised Sharpe (143 vs 0.7)
- Best pair: XRPUSDT (+90% annualised, Sharpe 6060, 14.1% max DD)

## Status

**Phase 1 Complete:** Backtest engine with slippage, trailing stops, short selling
**Phase 2 Complete:** Passive execution (FDUSD 0% maker), ATR-based stops, normalised metrics
**Phase 3 In Progress:** FDUSD validation, live paper trading

See `plans/active/krypto-rethink.md` for full project roadmap.
