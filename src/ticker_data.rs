use crate::{
    candle::{Candle, TechnicalType, TECHNICAL_COUNT},
    krypto_error::DataError,
};
use getset::{Getters, MutGetters};
use std::{fs, path::Path};

#[derive(Debug, Clone, Getters, MutGetters)]
#[getset(get = "pub")]
pub struct TickerData {
    ticker: Box<str>,
    #[getset(get_mut = "pub")]
    candles: Box<[Candle]>,
}

impl TickerData {
    pub fn new(ticker: String, candles: Vec<Candle>) -> Self {
        Self {
            ticker: ticker.into_boxed_str(),
            candles: candles.into_boxed_slice(),
        }
    }

    pub fn ensure_validity(&self, other: &TickerData) -> Result<(), DataError> {
        if self.candles.len() != other.candles.len() {
            return Err(DataError::NotEnoughData(self.ticker().to_string()));
        }

        if self
            .candles
            .iter()
            .zip(other.candles.iter())
            .any(|(a, b)| check_valid(a.close_time(), b.close_time()) == false)
        {
            // Find the first time that doesn't match
            let (time1, time2) = self
                .candles
                .iter()
                .zip(other.candles.iter())
                .find(|(a, b)| check_valid(a.close_time(), b.close_time()) == false)
                .map(|(a, b)| (a.close_time(), b.close_time()))
                .unwrap();
            return Err(DataError::DataTimeMismatch(
                self.ticker().to_string(),
                other.ticker().to_string(),
                *time1,
                *time2,
            ));
        }

        Ok(())
    }

    fn csv_headers() -> Vec<String> {
        let mut headers = vec![
            "Open",
            "High",
            "Low",
            "Close",
            "Volume",
            "Percentage Change",
            "Date",
        ]
        .into_iter()
        .map(String::from)
        .collect::<Vec<String>>();

        headers.extend((0..TECHNICAL_COUNT).map(|i| TechnicalType::from_index(i).get_string()));
        headers
    }

    pub async fn print_to_file(&self, folder: &str) -> Result<(), std::io::Error> {
        let file_name = format!("{}/{}.csv", folder, self.ticker);
        let path = Path::new(&file_name);

        if !path.exists() {
            fs::create_dir_all(folder)?;
        }

        let mut file = fs::File::create(&file_name)?;
        let mut writer = csv::Writer::from_writer(&mut file);

        writer.write_record(Self::csv_headers())?;
        for candle in self.candles.iter() {
            let row = candle.serialize_to_csv_row();
            writer.write_record(row.split(','))?;
        }

        Ok(())
    }

    pub fn find_nan(&self) -> Result<(), DataError> {
        for candle in self.candles.iter() {
            candle.find_nan().map_err(|_| DataError::TickerHasNaN {
                ticker: self.ticker().to_string(),
            })?;
        }
        Ok(())
    }

    pub fn average_variance_score(&self) -> f64 {
        let mut sum = 0.0;
        for candle in self.candles.iter() {
            sum += *candle.variance_score() as f64;
        }
        sum / self.candles.len() as f64
    }
}

// 10 seconds
const MAX_TIME_DIFFERENCE: i64 = 10_000;

#[inline(always)]
pub fn check_valid(time_1: &i64, time_2: &i64) -> bool {
    (time_1 - time_2).abs() <= MAX_TIME_DIFFERENCE
}
