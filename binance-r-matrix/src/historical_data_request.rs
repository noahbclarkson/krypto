use binance::{market::Market, rest_model::{KlineSummaries, KlineSummary}};
use chrono::Utc;

use crate::{errors::DataError, HistoricalDataConfig, TickerData};

const MINS_TO_MILLIS: i64 = 60_000;

pub struct HistoricalDataRequest<'a> {
    start_time: i64,
    end_time: i64,
    config: &'a HistoricalDataConfig,
}

impl<'a> HistoricalDataRequest<'a> {
    pub fn new(config: &'a HistoricalDataConfig) -> Self {
        let end_time = Utc::now().timestamp_millis();
        let interval_minutes = config.interval_minutes() * config.periods;
        let start_time = end_time - (interval_minutes as i64 * MINS_TO_MILLIS);
        Self {
            start_time,
            end_time,
            config,
        }
    }

    pub async fn run(&self, ticker: &String) -> Result<TickerData, DataError> {
        let mut candlesticks = Vec::new();
        let addition = MINS_TO_MILLIS * 1000 * self.config.interval_minutes() as i64;
        let mut start_times = Vec::new();
        let mut start = self.start_time;
        while start < self.end_time {
            start_times.push(start as u64);
            start = start + addition;
        }

        let tasks = start_times
            .into_iter()
            .map(|s| self.load_chunk(ticker, s, s + addition as u64));
        
        let mut results = futures::future::join_all(tasks).await;
        for result in results.drain(..) {
            let summaries = result?;
            candlesticks.extend(summaries.into_iter().map(|summary| summary.into()));
        }

        let ticker_data = TickerData::new(ticker.clone(), candlesticks);
        Ok(ticker_data)
    }

    async fn load_chunk(
        &self,
        ticker: &String,
        start: u64,
        end: u64,
    ) -> Result<Vec<KlineSummary>, DataError> {
        let market: Market = self.config.get_binance();
        let summaries = market
            .get_klines(
                ticker.clone(),
                self.config.interval_string(),
                1000,
                Some(start),
                Some(end),
            )
            .await
            .map_err(|error| DataError::BinanceError {
                symbol: ticker.clone(),
                error,
            });
        let result = match summaries? {
            KlineSummaries::AllKlineSummaries(summaries) => summaries,
        };
        Ok(result)
    }
}
