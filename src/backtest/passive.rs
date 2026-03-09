//! Passive execution model for FDUSD pairs (0% maker fees).
//!
//! Instead of executing at market (paying taker fees), this module simulates
//! placing limit orders below each 1m candle's open and walking forward.
//!
//! # How it works (correct model)
//!
//! For each higher-timeframe bar (e.g., 1h):
//! 1. Iterate through each 1m candle within that bar
//! 2. At each 1m open, place limit at `open - N_ticks`
//! 3. If `low <= limit`: filled at limit (0 fees, better price!)
//! 4. If not filled: move to next 1m candle, update limit to new `open - N_ticks`
//! 5. Repeat until filled (almost 100% fill rate since most candles dip below open)
//!
//! # Example
//!
//! ```no_run
//! use krypto::backtest::passive::{PassiveExecutor, PassiveConfig, TickSize};
//! use krypto::data::loader::DataLoader;
//!
//! async fn example() -> anyhow::Result<()> {
//!     let loader = DataLoader::new(None, None);
//!     
//!     let df_1h = loader.fetch_data("BTCFDUSD", "1h", 1000).await?;
//!     let df_1m = loader.fetch_data("BTCFDUSD", "1m", 60000).await?;
//!     
//!     let signals = df_1h.column("close")?.f64()?.clone(); // Your strategy signals
//!     
//!     // Fetch tick size from API
//!     let tick_size = TickSize::fetch("BTCFDUSD").await?;
//!     
//!     let config = PassiveConfig {
//!         ticks_below_open: 3,      // 3 ticks below each 1m open
//!         tick_size,
//!         max_wait_bars: 60,        // Max 60 1m candles (1 hour)
//!         maker_fee: 0.0,           // FDUSD = 0%
//!     };
//!     
//!     let executor = PassiveExecutor::new(config);
//!     let fills = executor.simulate(&df_1h, &df_1m, &signals.into()).await?;
//!     
//!     println!("Fill rate: {:.1}%", fills.fill_rate * 100.0);
//!     Ok(())
//! }
//! ```

use anyhow::Result;
use polars::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Tick size configuration - fetched from Binance API
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TickSize(f64);

impl TickSize {
    /// Fetch tick size from Binance API.
    pub async fn fetch(symbol: &str) -> Result<Self> {
        let url = format!("https://fapi.binance.com/fapi/v1/exchangeInfo");
        let resp = reqwest::get(url).await?;
        
        let text = resp.text()?;
        let info: serde_json::Value = text;
        
        for sym_info in info["symbols"].as_array() {
            if sym_info["symbol"] == symbol {
                // Find PRICE_FILTER
                if let Some(filters) = sym_info.get("filters") {
                    if let Some(price_filter) = filters.iter().find(|f| f.get("filterType") == "PRICE_FILTER") {
                        if let Some(tick_str) = price_filter.get("tickSize") {
                            if let Ok(tick) = tick_str.parse::<f64>() {
                                return Ok(Self(tick));
                            }
                        }
                    }
                }
            }
        }
        
        // Fallback to hardcoded
        let tick = match symbol {
            s if s.starts_with("BTC") => 0.01,
            s if s.starts_with("ETH") => 0.001,
            s if s.starts_with("SOL") => 0.0001,
            s if s.starts_with("BNB") => 0.01,
            s if s.starts_with("XRP") => 0.00001,
            _ => 0.0001,
        };
        Self(tick)
    }

    /// Get the tick size value.
    pub fn value(&self) -> f64 {
        self.0
    }
}

