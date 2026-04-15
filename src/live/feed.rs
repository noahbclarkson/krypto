//! WebSocket data feed for live market data.
//!
//! Provides real-time kline data from Binance via WebSocket.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;

/// A kline event from Binance WebSocket.
#[derive(Debug, Clone, Deserialize)]
pub struct KlineEvent {
    #[serde(rename = "e")]
    pub event_type: String,
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "k")]
    pub kline: KlineData,
}

/// Kline data within a KlineEvent.
#[derive(Debug, Clone, Deserialize)]
pub struct KlineData {
    #[serde(rename = "t")]
    pub start_time: i64,
    #[serde(rename = "T")]
    pub end_time: i64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "i")]
    pub interval: String,
    #[serde(rename = "o")]
    pub open: String,
    #[serde(rename = "c")]
    pub close: String,
    #[serde(rename = "h")]
    pub high: String,
    #[serde(rename = "l")]
    pub low: String,
    #[serde(rename = "v")]
    pub volume: String,
    #[serde(rename = "x")]
    pub is_closed: bool,
}

impl KlineData {
    /// Parse open price as f64.
    pub fn open_f64(&self) -> Result<f64> {
        self.open.parse().context("Failed to parse open price")
    }

    /// Parse close price as f64.
    pub fn close_f64(&self) -> Result<f64> {
        self.close.parse().context("Failed to parse close price")
    }

    /// Parse high price as f64.
    pub fn high_f64(&self) -> Result<f64> {
        self.high.parse().context("Failed to parse high price")
    }

    /// Parse low price as f64.
    pub fn low_f64(&self) -> Result<f64> {
        self.low.parse().context("Failed to parse low price")
    }

    /// Parse volume as f64.
    pub fn volume_f64(&self) -> Result<f64> {
        self.volume.parse().context("Failed to parse volume")
    }

    /// Get start time as DateTime.
    pub fn start_datetime(&self) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(self.start_time).unwrap_or_default()
    }

    /// Convert to a Bar for use with strategies.
    pub fn to_bar(&self) -> Result<crate::paper::Bar> {
        Ok(crate::paper::Bar::new(
            self.start_datetime(),
            self.open_f64()?,
            self.high_f64()?,
            self.low_f64()?,
            self.close_f64()?,
            self.volume_f64()?,
        ))
    }
}

/// Combined stream event for multiple symbols.
#[derive(Debug, Clone, Deserialize)]
pub struct CombinedStreamEvent {
    pub stream: String,
    pub data: KlineEvent,
}

/// Live data feed using Binance WebSocket.
pub struct LiveFeed {
    symbols: Vec<String>,
    interval: String,
    running: Arc<AtomicBool>,
    sender: broadcast::Sender<KlineEvent>,
}

impl LiveFeed {
    /// Create a new live feed for the given symbols and interval.
    pub fn new(symbols: Vec<String>, interval: String) -> Self {
        let (sender, _) = broadcast::channel(256);
        Self {
            symbols,
            interval,
            running: Arc::new(AtomicBool::new(false)),
            sender,
        }
    }

    /// Subscribe to kline events.
    pub fn subscribe(&self) -> broadcast::Receiver<KlineEvent> {
        self.sender.subscribe()
    }

    /// Start the WebSocket connection.
    ///
    /// This spawns a background task that connects to Binance and
    /// broadcasts kline events to subscribers.
    pub async fn start(&self) -> Result<()> {
        use binance::errors::Result as BinanceResult;
        use binance::websockets::WebSockets;

        self.running.store(true, Ordering::SeqCst);
        let running = self.running.clone();
        let sender = self.sender.clone();

        // Build stream names for all symbols
        let streams: Vec<String> = self
            .symbols
            .iter()
            .map(|s| format!("{}@kline_{}", s.to_lowercase(), self.interval))
            .collect();

        tracing::info!("Starting WebSocket feed for {} streams", streams.len());

        // Handler for incoming events
        let handler = move |event: CombinedStreamEvent| -> BinanceResult<()> {
            if event.data.kline.is_closed {
                // Only broadcast closed candles (completed bars)
                if let Err(e) = sender.send(event.data) {
                    tracing::warn!("Failed to broadcast kline event: {}", e);
                }
            }
            Ok(())
        };

        let mut ws: WebSockets<'_, CombinedStreamEvent> = WebSockets::new(handler);

        // Connect to combined stream
        ws.connect_multiple(streams.clone())
            .await
            .context("Failed to connect to WebSocket")?;

        tracing::info!("WebSocket connected to {} streams", streams.len());

        // Run event loop in background
        tokio::spawn(async move {
            if let Err(e) = ws.event_loop(&running).await {
                tracing::error!("WebSocket error: {}", e);
            }
        });

        Ok(())
    }

    /// Stop the WebSocket connection.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Check if the feed is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

/// Fetch historical data for warmup before starting live feed.
pub async fn fetch_warmup_data(
    symbol: &str,
    interval: &str,
    bars: usize,
) -> Result<Vec<crate::paper::Bar>> {
    use crate::data::loader::DataLoader;

    let loader = DataLoader::new(None, None);
    let df = loader
        .fetch_data(symbol, interval, bars as u32)
        .await
        .context("Failed to fetch warmup data")?;

    let mut result = Vec::with_capacity(bars);

    let time = df.column("time")?.datetime()?;
    let open = df.column("open")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let close = df.column("close")?.f64()?;
    let volume = df.column("volume")?.f64()?;

    for i in 0..df.height() {
        if let (Some(t), Some(o), Some(h), Some(l), Some(c), Some(v)) = (
            time.get(i),
            open.get(i),
            high.get(i),
            low.get(i),
            close.get(i),
            volume.get(i),
        ) {
            result.push(crate::paper::Bar::new(
                DateTime::from_timestamp_millis(t).unwrap_or_default(),
                o,
                h,
                l,
                c,
                v,
            ));
        }
    }

    tracing::info!("Fetched {} bars for warmup ({})", result.len(), symbol);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kline_data_parsing() {
        let json = r#"{
            "t": 1672531200000,
            "T": 1672617599999,
            "s": "BTCFDUSD",
            "i": "1d",
            "o": "16500.00",
            "c": "16800.00",
            "h": "16900.00",
            "l": "16400.00",
            "v": "1000.50",
            "x": true
        }"#;

        let kline: KlineData = serde_json::from_str(json).unwrap();
        assert_eq!(kline.symbol, "BTCFDUSD");
        assert_eq!(kline.interval, "1d");
        assert!((kline.open_f64().unwrap() - 16500.0).abs() < 0.01);
        assert!((kline.close_f64().unwrap() - 16800.0).abs() < 0.01);
        assert!(kline.is_closed);
    }

    #[test]
    fn test_kline_event_parsing() {
        let json = r#"{
            "e": "kline",
            "E": 1672531200000,
            "s": "BTCFDUSD",
            "k": {
                "t": 1672531200000,
                "T": 1672617599999,
                "s": "BTCFDUSD",
                "i": "1d",
                "o": "16500.00",
                "c": "16800.00",
                "h": "16900.00",
                "l": "16400.00",
                "v": "1000.50",
                "x": true
            }
        }"#;

        let event: KlineEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type, "kline");
        assert_eq!(event.symbol, "BTCFDUSD");
    }

    #[test]
    fn test_live_feed_creation() {
        let feed = LiveFeed::new(vec!["BTCFDUSD".to_string()], "1d".to_string());
        assert!(!feed.is_running());
        assert_eq!(feed.symbols.len(), 1);
    }
}
