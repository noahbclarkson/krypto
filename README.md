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

## Status

**Phase 1 Complete:** Backtest engine with slippage and trailing stop fixes
**Phase 2 In Progress:** Experiment foundation and research loop

See `plans/active/krypto-rethink.md` for full project roadmap.
