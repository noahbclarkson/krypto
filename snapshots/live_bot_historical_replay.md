# Live Bot Historical Replay

Production `LiveBot::process_bar()` replay against cached aligned daily bars.

| Metric | Value |
|---|---:|
| Symbols | BTCUSDT, ETHUSDT, SOLUSDT, XRPUSDT, DOGEUSDT, ADAUSDT |
| Common aligned days | 2101 |
| Warmup bars seeded per symbol | 300 |
| Replayed date range | 2021-06-07 → 2026-05-12 |
| Processed closed-bar events | 10806 |
| Closed trades recorded by LiveBot | 286 |
| State trades | 286 |
| Open positions at end | 0 |
| Dry run | true |

## Interpretation

This harness closes the no-API replay gap at the production event path level: cached bars can now drive `src/live/bot.rs` directly without WebSocket/testnet credentials.

It is **not** a replacement for `live_bot_exact_equity.rs`: live `BotState.equity` is operational monitoring state, not the compounding account-equity source of truth. Use this replay to catch production path breakage; use exact-live equity for performance metrics.
