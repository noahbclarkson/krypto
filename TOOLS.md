# TOOLS.md - Local Notes

## Git Push Fix

If `git push` fails with "Invalid username or token", run:
```bash
unset GH_TOKEN && git push
```
The stale `GH_TOKEN` env var overrides the working `gh auth git-credential` helper. Unsetting it fixes the issue.

## Repo

- **Path:** `~/.openclaw/workspace-krypto/krypto/` (absolute)
- **Branch:** `v2-rewrite`
- **Build:** `cd ~/.openclaw/workspace-krypto/krypto && cargo build`
- **Run example:** `cd ~/.openclaw/workspace-krypto/krypto && cargo run --example <name> --profile sweep`
- **Run sweep profile** (fast): `--profile sweep` (opt-level 1, no LTO)

## Discord

- Channel: #krypto (`1484783323497762816`) — send progress updates here
- Guild: `1484743728722743440`

## Data

- **Cache dir:** `krypto/data/cache/` — parquet files already downloaded
- **Funding cache:** `krypto/examples/funding_cache/`
- **Binance data loader:** `src/data/loader.rs` — fetches from Binance API

## Key Crate Features

- Polars 0.37 with parquet, lazy, temporal, rolling_window, ewma
- binance-rs-async for market data
- plotters for charts (bitmap backend)
- rayon for parallelism

## LaTeX / PDF Report Generation

TeX is installed on the VPS. Use this for research write-ups and reports.

```bash
which pdflatex  # /usr/bin/pdflatex
which lualatex  # /usr/bin/lualatex
```

Packages available: texlive-latex-base, texlive-latex-extra, texlive-pictures, texlive-science, texlive-fonts-recommended.

To compile a LaTeX document:
```bash
pdflatex -interaction=nonstopmode report.tex
# Run twice for TOC/refs to resolve
pdflatex -interaction=nonstopmode report.tex
```

## Known Working Examples

- `bollinger_1d_sweep` — best validated strategy sweep
- `bollinger_portfolio` — 5-symbol portfolio backtest
- `bollinger_oos_validation` — OOS time-split validation
- `walk_forward` — walk-forward validation harness
- `passive_backtest` — passive limit order execution simulation
- `strategy_sweep` — broad strategy parameter sweep
- `hall_of_fame_validator` — validates all HOF strategies
