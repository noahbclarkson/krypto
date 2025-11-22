use anyhow::Result;
use binance::api::Binance;
use binance::market::Market;
use binance::rest_model::KlineSummaries;
use chrono::DateTime;
use polars::prelude::*;

pub struct DataLoader {
    market: Market,
}

impl DataLoader {
    pub fn new(api_key: Option<String>, secret_key: Option<String>) -> Self {
        Self {
            market: Market::new(api_key, secret_key),
        }
    }

    /// Fetches candles with pagination (1000 max per request on Binance).
    pub async fn fetch_data(
        &self,
        symbol: &str,
        interval: &str,
        total_candles: u16,
    ) -> Result<DataFrame> {
        let mut all_klines = Vec::new();
        let mut remaining = total_candles as usize;
        let mut end_time: Option<u64> = None;

        while remaining > 0 {
            let fetch_limit = remaining.min(1000) as u16;
            let resp = self
                .market
                .get_klines(symbol, interval, Some(fetch_limit), None, end_time)
                .await?;

            let KlineSummaries::AllKlineSummaries(mut batch) = resp;
            if batch.is_empty() {
                break;
            }

            // Prepare for older page: ask for klines ending before the earliest we just got
            end_time = batch.first().map(|k| k.open_time.saturating_sub(1) as u64); // avoid overlap

            remaining = remaining.saturating_sub(batch.len());
            all_klines.append(&mut batch);

            // small pause to avoid rate limits
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        // Oldest -> newest and dedupe any overlap
        all_klines.sort_by_key(|k| k.open_time);
        all_klines.dedup_by_key(|k| k.open_time);

        let mut open_times = Vec::with_capacity(all_klines.len());
        let mut opens = Vec::with_capacity(all_klines.len());
        let mut highs = Vec::with_capacity(all_klines.len());
        let mut lows = Vec::with_capacity(all_klines.len());
        let mut closes = Vec::with_capacity(all_klines.len());
        let mut volumes = Vec::with_capacity(all_klines.len());

        for k in all_klines {
            let secs = k.open_time / 1000;
            let nsecs = ((k.open_time % 1000) * 1_000_000) as u32;
            if let Some(dt) = DateTime::from_timestamp(secs, nsecs) {
                open_times.push(dt.naive_utc());
            }
            opens.push(k.open);
            highs.push(k.high);
            lows.push(k.low);
            closes.push(k.close);
            volumes.push(k.volume);
        }

        let df = df!(
            "time" => open_times,
            "open" => opens,
            "high" => highs,
            "low" => lows,
            "close" => closes,
            "volume" => volumes
        )?;

        Ok(df)
    }
}
