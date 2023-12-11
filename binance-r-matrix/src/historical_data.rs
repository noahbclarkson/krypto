use getset::Getters;
use r_matrix::{dataset::DatasetBuilder, Dataset};

use crate::{
    config::HistoricalDataConfig, error::BinanceDataError,
    historical_data_request::HistoricalDataRequest, math::percentage_change,
    technical_calculator::BinanceDataType, ticker_data::TickerData,
};

#[derive(Debug, Getters)]
#[getset(get = "pub")]
pub struct HistoricalData {
    data: Vec<TickerData>,
    config: HistoricalDataConfig,
}

impl HistoricalData {
    pub fn new(config: HistoricalDataConfig) -> Self {
        Self {
            data: Vec::new(),
            config,
        }
    }

    pub async fn load(&mut self) -> Result<(), BinanceDataError> {
        let request = HistoricalDataRequest::new(&self.config);
        let tasks = self
            .config
            .tickers()
            .iter()
            .map(|ticker| request.run(ticker));
        let tickers = futures::future::join_all(tasks).await;
        self.data = tickers.into_iter().collect::<Result<Vec<_>, _>>()?;
        self.validate()?;
        Ok(())
    }

    fn validate(&mut self) -> Result<(), BinanceDataError> {
        let close_time_variance = self.config.interval_minutes() * 60 * 300;
        for t_data in self.data.iter() {
            t_data.validate(*self.config.periods())?;
        }
        let close_times = self.data[0].close_times().collect::<Vec<_>>();
        for t_data in self.data.iter().skip(1) {
            let close_times_2 = t_data.close_times();
            for (time_1, time_2) in close_times.iter().zip(close_times_2) {
                if (time_1 - time_2).abs() > close_time_variance as i64 {
                    return Err(BinanceDataError::MismatchedCloseTimes {
                        symbol: t_data.ticker().to_string(),
                        time_1: *time_1,
                        time_2,
                    });
                }
            }
        }
        Ok(())
    }

    pub fn calculate_technicals(&mut self) -> Result<(), BinanceDataError> {
        for t_data in self.data.iter_mut() {
            t_data.load_technicals()?;
        }
        Ok(())
    }

    pub fn to_dataset(&self) -> Dataset {
        let mut dataset = DatasetBuilder::default();

        if self.data.is_empty() || self.data[0].technicals().is_empty() {
            return dataset.build().unwrap(); // Return an empty dataset if no data is present
        }

        for i in 0..self.data[0].technicals().len() {
            let mut features: Vec<f64> = Vec::new();
            let mut labels: Vec<f64> = Vec::new();

            for t_data in &self.data {
                let technicals = t_data.technicals();
                let tech = technicals[i].to_vec();
                features.extend(tech);

                // Add the label
                let last_close = t_data
                    .klines()
                    .get(i - 1)
                    .map(|kline| kline.close)
                    .unwrap_or(t_data.klines()[i].close);
                let label = t_data
                    .klines()
                    .get(i)
                    .map(|kline| percentage_change(last_close, kline.close))
                    .unwrap_or(0.0);
                labels.push(label);
            }

            dataset.add_data_point(i, features, labels);
        }

        let mut feature_names = Vec::new();
        let mut label_names = Vec::new();

        for t_data in &self.data {
            feature_names.extend(
                BinanceDataType::get_feature_names()
                    .iter()
                    .filter(|&name| *name != "PercentageChange")
                    .map(|name| format!("{}_{}", t_data.ticker(), name)),
            );
            label_names.push(format!("PercentageChange_{}", t_data.ticker()));
        }

        dataset.set_feature_names(feature_names);
        dataset.set_label_names(label_names);

        dataset.build().unwrap()
    }
}
