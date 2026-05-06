# T53 Mock Exchange Cost Smoke — 2026-05-06

## Why

`examples/mock_live_bot_v2.rs` previously reported identical economics for TAKER / MAKER / REALISTIC configurations because cash/equity accounting ignored actual `MockExchange` fill prices, fees, and slippage.

This session fixes that specific audit failure. It does **not** complete the full T53 end-to-end goal (`src/live/bot.rs` wired to a mock feed/executor), but it turns the existing daily-bar mock harness into a valid execution-cost smoke test.

## Change

- `simulate_symbol` now advances `MockExchange` first for each daily bar.
- Entry/exit orders are submitted as market-at-close orders against the just-closed bar.
- Cash/equity now mutates from `MockExchange::fills()`:
  - BUY: subtract fill price × qty + fee
  - SELL: add fill price × qty − fee
  - mark-to-market uses current close for open positions
- Report now prints total fees and modeled slippage cost.
- Removed bogus maker-fill percentage accounting from the harness.

## Validation command

```bash
cargo run --example mock_live_bot_v2 --profile sweep
```

## Results

| Config | Avg Return | Sharpe | MaxDD | Trades | Fees | Slippage |
|---|---:|---:|---:|---:|---:|---:|
| TAKER 4bps + 5bp slip | +161.8% | 0.89 | 16.8% | 300 | $835.63 | $1,044.56 |
| MAKER 2bps + 2bp slip | +163.9% | 0.91 | 16.6% | 300 | $417.82 | $417.82 |
| REALISTIC 2.8bps + 3bp slip | +163.2% | 0.90 | 16.6% | 300 | $584.95 | $626.74 |

## Verdict

The prior identical-config result is fixed. Execution costs now move the equity curve in the expected direction. The gap between optimistic and pessimistic daily-bar execution is about **2.1 percentage points of average return** across Base5 in this smoke test.

## Remaining T53 gaps

- Still not an end-to-end `src/live/bot.rs` wire replacement.
- Still no local HTTP/WebSocket mock server.
- Still daily-bar only; cached 1m parquet is absent.
- Market-at-close daily bars are only a cost smoke test, not real maker/limit-order microstructure evidence.
