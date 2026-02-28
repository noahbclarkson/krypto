# Data Loading Pipeline

This document describes the data loading and processing pipeline in Krypto.

## Overview

```
┌─────────────┐     ┌──────────────┐     ┌────────────────┐     ┌───────────────┐
│   Binance   │ ──► │  DataLoader  │ ──► │ FeatureEngine  │ ──► │   Backtest    │
│     API     │     │              │     │                │     │    Engine     │
└─────────────┘     └──────────────┘     └────────────────┘     └───────────────┘
     Raw JSON         Polars DF            Technical           Strategy Signals
     Klines           OHLCV               Indicators           & Equity Curve
```

## 1. Data Source: Binance API

**Endpoint**: `GET /api/v3/klines`

**Parameters**:
- `symbol`: Trading pair (e.g., `BTCUSDT`)
- `interval`: Candle interval (e.g., `1h`, `4h`, `1d`)
- `limit`: Number of candles (max 1000 per request)
- `startTime`: Optional start timestamp (ms)
- `endTime`: Optional end timestamp (ms)

**Response Format** (array of arrays):
```json
[
  [
    1772204400000,      // 0: Open time (ms)
    "65887.59",         // 1: Open
    "66311.11",         // 2: High
    "65845.18",         // 3: Low
    "66095.40",         // 4: Close
    "957.11978",        // 5: Volume
    1772207999999,      // 6: Close time (ms)
    "63243068.11",      // 7: Quote asset volume
    257350,             // 8: Number of trades
    "483.47",           // 9: Taker buy base volume
    "31947647.80",      // 10: Taker buy quote volume
    "0"                 // 11: Ignore
  ],
  // ... more candles
]
```

## 2. DataLoader (`src/data/loader.rs`)

The `DataLoader` struct wraps the `binance-rs-async` crate and provides:

### Constructor
```rust
DataLoader::new(api_key: Option<String>, secret_key: Option<String>)
```
- Public endpoints don't require API keys
- Keys only needed for private endpoints (trading, account)

### Main Method: `fetch_data`
```rust
async fn fetch_data(
    &self,
    symbol: &str,      // e.g., "BTCUSDT"
    interval: &str,    // e.g., "1h", "4h", "1d"
    total_candles: u16 // Max 65535 candles
) -> Result<DataFrame>
```

### Pagination Logic

1. **Batch fetching**: Requests up to 1000 candles per API call
2. **Backward pagination**: Uses `endTime` to fetch older candles
3. **Deduplication**: Sorts by `open_time` and removes duplicates
4. **Rate limiting**: 100ms delay between requests

```rust
while remaining > 0 {
    let fetch_limit = remaining.min(1000) as u16;
    let resp = self.market.get_klines(
        symbol, interval, 
        Some(fetch_limit), 
        None, 
        end_time
    ).await?;
    
    // Prepare for next (older) page
    end_time = batch.first().map(|k| k.open_time.saturating_sub(1));
    remaining = remaining.saturating_sub(batch.len());
    
    tokio::time::sleep(Duration::from_millis(100)).await;
}
```

### Output DataFrame Schema

| Column   | Type             | Description              |
|----------|------------------|--------------------------|
| `time`   | `NaiveDateTime`  | UTC open time            |
| `open`   | `f64`            | Opening price            |
| `high`   | `f64`            | Highest price            |
| `low`    | `f64`            | Lowest price             |
| `close`  | `f64`            | Closing price            |
| `volume` | `f64`            | Base asset volume        |

## 3. Feature Engineering (`src/features/indicators.rs`)

The `FeatureEngine` adds technical indicators to the raw OHLCV data:

### `add_technicals(df: &DataFrame, config: Option<FeatureConfig>) -> Result<DataFrame>`

**Indicators Added**:
- `rsi`: Relative Strength Index (14-period)
- `atr`: Average True Range (14-period)
- `macd`: MACD line (12, 26, 9)
- `macd_signal`: MACD signal line
- `macd_hist`: MACD histogram
- `ema_20`: 20-period EMA
- `ema_50`: 50-period EMA
- `ema_200`: 200-period EMA (if enough data)
- `bb_upper`, `bb_lower`, `bb_mid`: Bollinger Bands
- `volatility`: Rolling volatility

## 4. Experiment Runner Integration (`src/experiment/runner.rs`)

The `ExperimentRunner` orchestrates the full data flow:

```rust
// 1. Load raw data
let raw_data = self.load_data_for_symbol(symbol)?;

// 2. Add features
let data = FeatureEngine::add_technicals(&raw_data, None)?;

// 3. Split into train/test
let train_df = data.slice(train_start, train_len);
let test_df = data.slice(test_start, test_len);

// 4. Generate signals
let signals = strategy.predict(&df)?;

// 5. Run backtest
let result = backtester.run(&df, &signals, trailing_stop)?;
```

### Lookback Calculation

The runner calculates how many candles to fetch based on:

1. **Explicit**: `config.data.lookback_candles` if set
2. **Date range**: `(end_date - start_date) / interval_seconds`

```rust
fn resolve_lookback_candles(&self) -> Result<u16> {
    if let Some(lookback) = self.config.data.lookback_candles {
        return Ok(lookback.min(u16::MAX as usize) as u16);
    }
    // Calculate from date range...
}
```

## Sample Data

Sample data is available in `data/samples/`:

- `btcusdt_1h_500candles.json`: 500 hourly candles for BTCUSDT

## Testing the Pipeline

### Quick API Test
```bash
curl -s "https://api.binance.com/api/v3/klines?symbol=BTCUSDT&interval=1h&limit=10" | jq '.'
```

### Rust Example
```bash
cargo run --example basic_trading
```

This will:
1. Fetch 1000 hourly candles for BTCUSDT
2. Apply fractional differentiation
3. Generate triple-barrier labels
4. Run ensemble analysis

## Error Handling

The pipeline handles these error cases:

| Error                 | Handling                          |
|-----------------------|-----------------------------------|
| No data returned      | Returns error with symbol name    |
| Invalid interval      | Fails with supported intervals    |
| Rate limit hit        | Built-in 100ms delay per request  |
| Network timeout       | Propagates via `anyhow::Result`   |
| Invalid JSON response | Handled by `binance-rs-async`     |

## Supported Intervals

| Interval | Seconds  |
|----------|----------|
| `1m`     | 60       |
| `3m`     | 180      |
| `5m`     | 300      |
| `15m`    | 900      |
| `30m`    | 1800     |
| `1h`     | 3600     |
| `2h`     | 7200     |
| `4h`     | 14400    |
| `6h`     | 21600    |
| `8h`     | 28800    |
| `12h`    | 43200    |
| `1d`     | 86400    |
| `3d`     | 259200   |
| `1w`     | 604800   |

## Rate Limits

Binance API limits:
- **Weight**: 1200 per minute (default)
- **Klines weight**: 1 per request
- **Recommendation**: Stay under 1000 requests/minute

The `DataLoader` adds 100ms delay between paginated requests (~600/minute).

## Future Improvements

1. **Caching**: Store fetched data locally to avoid re-fetching
2. **Parallel fetching**: Fetch multiple symbols concurrently
3. **WebSocket streaming**: Real-time data via WebSocket connection
4. **Multiple sources**: Add Coinbase, Kraken, etc.
5. **Resumable fetches**: Handle interruptions gracefully
