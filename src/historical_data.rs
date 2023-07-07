use std::error::Error;

use binance::{api::Binance, market::Market, rest_model::KlineSummaries};
use getset::{Getters, MutGetters};
use ta::{
    indicators::{CommodityChannelIndex, RelativeStrengthIndex, SlowStochastic},
    Next,
};

use crate::{
    candlestick::{Candlestick, TechnicalType::*, TECHNICAL_COUNT},
    config::Config,
    math::{cr_ratio, percentage_change},
};

pub const MINUTES_TO_MILLIS: i64 = 60_000;

#[derive(Debug, Getters, MutGetters)]
pub struct TickerData {
    #[getset(get = "pub")]
    ticker: Box<str>,
    #[getset(get = "pub", get_mut = "pub")]
    candles: Box<[Candlestick]>,
}

impl TickerData {
    pub fn new(ticker: String, candles: Vec<Candlestick>) -> Self {
        Self {
            ticker: Box::from(ticker),
            candles: Box::from(candles),
        }
    }
}

pub async fn load(
    config: &Config,
    tickers: Vec<String>,
) -> Result<Box<[TickerData]>, Box<dyn Error>> {
    let current_time = chrono::Utc::now().timestamp_millis();
    let interval_minutes = config.interval_minutes()? * *config.periods() as i64;
    let start_time = current_time - interval_minutes * MINUTES_TO_MILLIS;

    let tasks = tickers
        .into_iter()
        .map(|ticker| load_ticker(ticker, start_time, current_time, config));

    let tickers = futures::future::join_all(tasks).await;
    let tickers = tickers.into_iter().collect::<Result<Vec<_>, _>>()?;
    let candles: Box<[TickerData]> = Box::from(tickers);
    futures::executor::block_on(check_data(candles.as_ref(), config))?;
    Ok(candles)
}

async fn load_ticker(
    ticker: String,
    start_time: i64,
    current_time: i64,
    config: &Config,
) -> Result<TickerData, Box<dyn Error>> {
    let mut candlesticks = Vec::new();
    let market: Market = Binance::new(config.api_key().clone(), config.api_secret().clone());
    let addition = MINUTES_TO_MILLIS * 1000 * config.interval_minutes()?;
    let mut start_time = start_time;
    let mut start_times = Vec::new();

    while start_time < current_time {
        let end_time = start_time + addition;
        start_times.push(start_time as u64);
        start_time = end_time;
    }

    let tasks = start_times.into_iter().map(|start_time| {
        load_chunk(
            ticker.clone(),
            start_time,
            start_time + addition as u64,
            config,
            &market,
        )
    });
    let results = futures::future::join_all(tasks).await;

    for result in results {
        let chunk = result?;
        candlesticks.extend(chunk.into_iter());
    }

    candlesticks.sort_by(|a, b| a.close_time().cmp(b.close_time()));
    Ok(TickerData::new(ticker, candlesticks))
}

async fn load_chunk(
    ticker: String,
    start_time: u64,
    end_time: u64,
    config: &Config,
    market: &Market,
) -> Result<Vec<Candlestick>, Box<dyn Error>> {
    let summaries = market
        .get_klines(
            ticker.clone(),
            config.interval(),
            1000,
            Some(start_time as u64),
            Some(end_time as u64),
        )
        .await
        .map_err(|error| {
            Box::new(DataError::BinanceError {
                symbol: ticker,
                error,
            })
        })?;
    Ok(expand_summaries(summaries))
}

pub fn calculate_technicals(mut candles: Box<[TickerData]>) -> Box<[TickerData]> {
    let mut stoch = SlowStochastic::default();
    let mut rsi = RelativeStrengthIndex::default();
    let mut cci = CommodityChannelIndex::default();

    for ticker in candles.iter_mut() {
        let mut previous_close = ticker.candles()[0].close().clone();
        let mut previous_volume = ticker.candles()[0].volume().clone();

        for candle in ticker.candles_mut().iter_mut() {
            let p_change = percentage_change(previous_close, candle.close().clone());
            let v_change = percentage_change(previous_volume, candle.volume().clone());
            previous_close = candle.close().clone();
            previous_volume = candle.volume().clone();
            candle.technicals_mut()[PercentageChange as usize] = p_change;
            candle.set_p_change(p_change);
            candle.technicals_mut()[VolumeChange as usize] = v_change;

            let item = match candle.to_data_item() {
                Ok(data_item) => data_item,
                Err(_) => continue,
            };

            candle.technicals_mut()[CandlestickRatio as usize] = cr_ratio(&item);
            candle.technicals_mut()[StochasticOscillator as usize] =
                stoch.next(&item).round() as f32;
            candle.technicals_mut()[RelativeStrengthIndex as usize] =
                rsi.next(&item).round() as f32;
            candle.technicals_mut()[CommodityChannelIndex as usize] =
                cci.next(&item).round() as f32;
        }
    }

    let means = calculate_means(candles.as_ref());
    let stddevs = calculate_stddevs(candles.as_ref(), means);
    normalize(candles, means, stddevs)
}

fn calculate_means(candles: &[TickerData]) -> [f32; TECHNICAL_COUNT] {
    let mut means = [0.0; TECHNICAL_COUNT];
    for ticker in candles.into_iter() {
        for candle in ticker.candles().iter() {
            for (index, technical) in candle.technicals().iter().enumerate() {
                means[index] += technical;
            }
        }
    }
    let count = candles.len() * candles[0].candles().len();
    means.iter_mut().for_each(|mean| *mean /= count as f32);
    means
}

fn calculate_stddevs(
    candles: &[TickerData],
    means: [f32; TECHNICAL_COUNT],
) -> [f32; TECHNICAL_COUNT] {
    let mut stdev = [0.0; TECHNICAL_COUNT];
    for ticker in candles.into_iter() {
        for candle in ticker.candles().iter() {
            for (index, technical) in candle.technicals().iter().enumerate() {
                stdev[index] += (*technical - means[index]).powi(2);
            }
        }
    }
    let count = candles.len() * candles[0].candles().len();
    stdev
        .iter_mut()
        .for_each(|stdev| *stdev = (*stdev / count as f32).sqrt());
    stdev
}

fn normalize(
    mut candles: Box<[TickerData]>,
    means: [f32; TECHNICAL_COUNT],
    stddevs: [f32; TECHNICAL_COUNT],
) -> Box<[TickerData]> {
    for ticker in candles.iter_mut() {
        for candle in ticker.candles_mut().iter_mut() {
            for (index, technical) in candle.technicals_mut().iter_mut().enumerate() {
                *technical = (*technical - means[index]) / stddevs[index];
                if technical.is_nan() || technical.is_infinite() {
                    *technical = 0.0;
                }
            }
        }
    }
    candles
}

async fn check_data(candles: &[TickerData], config: &Config) -> Result<(), Box<dyn Error>> {
    check_length(candles, config).await?;
    check_times(candles).await?;
    Ok(())
}

async fn check_length(candles: &[TickerData], config: &Config) -> Result<(), Box<dyn Error>> {
    for ticker in candles.iter() {
        if ticker.candles.len() < *config.periods() {
            return Err(Box::new(DataError::NotEnoughData(
                ticker.ticker.to_string(),
            )));
        }
    }
    Ok(())
}

async fn check_times(candles: &[TickerData]) -> Result<(), Box<dyn Error>> {
    let first_ticker = &candles[0];
    let first_ticker_times: Vec<i64> = first_ticker
        .candles()
        .iter()
        .map(|c| *c.close_time())
        .collect();

    for ticker in candles.iter().skip(1) {
        let ticker_times: Vec<_> = ticker.candles.iter().map(|c| *c.close_time()).collect();
        if ticker_times != first_ticker_times {
            return Err(Box::new(DataError::DataTimeMismatch(
                ticker.ticker.to_string(),
            )));
        }
    }
    Ok(())
}

fn expand_summaries(summaries: KlineSummaries) -> Vec<Candlestick> {
    match summaries {
        KlineSummaries::AllKlineSummaries(summaries) => summaries
            .into_iter()
            .map(|summary| Candlestick::new_from_summary(summary))
            .collect(),
    }
}

#[derive(thiserror::Error, Debug)]
pub enum DataError {
    #[error("Not enough data for ticker {0}")]
    NotEnoughData(String),
    #[error("Data time mismatch for ticker {0}")]
    DataTimeMismatch(String),
    #[error("Binance error for symbol {symbol}: {error}")]
    BinanceError {
        symbol: String,
        error: binance::errors::Error,
    },
}
