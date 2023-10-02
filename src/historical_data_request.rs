use binance::{market::Market, rest_model::KlineSummaries};
use chrono::Utc;

use crate::{
    candle::Candle, config::Config, historical_data::MINS_TO_MILLIS, krypto_error::DataError,
    ticker_data::TickerData,
};

pub struct HistoricalDataRequest {
    start_time: i64,
    end_time: i64,
    config: Config,
}

impl HistoricalDataRequest {
    pub fn new(config: Config, interval_index: usize) -> Self {
        let end_time = Utc::now().timestamp_millis();
        let interval_minutes = (config.interval_minutes(interval_index) * config.periods() * config.depths()[interval_index]) as i64;
        let start_time = end_time - (interval_minutes * MINS_TO_MILLIS);
        Self {
            start_time,
            end_time,
            config,
        }
    }

    pub async fn run(&self, ticker: &String, interval_index: usize) -> Result<TickerData, DataError> {
        let mut candlesticks = Vec::new();
        let market: Market = self.config.get_binance();
        let addition = (MINS_TO_MILLIS * 1000 * self.config.interval_minutes(interval_index) as i64) as u64;
        let mut start_time = self.start_time;
        let mut start_times = Vec::new();

        while start_time < self.end_time {
            let end_time = start_time + addition as i64;
            start_times.push(start_time as u64);
            start_time = end_time;
        }

        let tasks = start_times
            .into_iter()
            .map(|s| self.load_chunk(ticker.clone(), (s, s + addition), &market, interval_index));
        let results = futures::future::join_all(tasks).await;

        for result in results {
            let chunk: Vec<Candle> = result?;
            candlesticks.extend(chunk);
        }

        candlesticks.sort_by(|a, b| a.close_time().cmp(b.close_time()));
        Ok(TickerData::new(ticker.clone(), candlesticks))
    }

    async fn load_chunk(
        &self,
        ticker: String,
        times: (u64, u64),
        market: &Market,
        interval_index: usize,
    ) -> Result<Vec<Candle>, DataError> {
        let summaries = market
            .get_klines(
                ticker.clone(),
                self.config.intervals()[interval_index].clone(),
                1000,
                Some(times.0),
                Some(times.1),
            )
            .await
            .map_err(|error| DataError::BinanceError {
                symbol: ticker,
                error,
            })?;
        Ok(expand_summaries(summaries))
    }

}

fn expand_summaries(summaries: KlineSummaries) -> Vec<Candle> {
    match summaries {
        KlineSummaries::AllKlineSummaries(summaries) => summaries
            .into_iter()
            .map(Candle::new_from_summary)
            .collect(),
    }
}