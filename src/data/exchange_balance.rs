//! Exchange balance / reserve loader.
//!
//! Current implementation targets CoinGlass's exchange balance chart endpoint.
//! It is intentionally trust-first:
//! - requires an explicit API key via `COINGLASS_API_KEY`
//! - caches successful responses locally
//! - fails honestly when the lane is blocked, instead of pretending the data exists

use anyhow::{anyhow, bail, Context, Result};
use polars::prelude::*;
use reqwest::Client;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

const COINGLASS_BALANCE_URL: &str = "https://open-api-v4.coinglass.com/api/exchange/balance/chart";

#[derive(Debug, Clone)]
pub struct ExchangeBalancePoint {
    pub time_ms: i64,
    pub price: f64,
    pub total_balance: f64,
}

#[derive(Debug, Clone)]
pub struct ExchangeBalanceLoader {
    client: Client,
    api_key: String,
    cache_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
struct CoinGlassResponse {
    code: String,
    msg: String,
    data: Vec<CoinGlassSeries>,
}

#[derive(Debug, Deserialize)]
struct CoinGlassSeries {
    time_list: Vec<i64>,
    price_list: Vec<f64>,
    data_map: BTreeMap<String, Vec<f64>>,
}

impl ExchangeBalanceLoader {
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("COINGLASS_API_KEY").context(
            "COINGLASS_API_KEY not set; exchange reserve / balance lane is currently blocked",
        )?;
        Ok(Self::new(api_key, "data/exchange_balance_cache"))
    }

    pub fn new(api_key: impl Into<String>, cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.into(),
            cache_dir: cache_dir.into(),
        }
    }

    fn cache_path(&self, symbol: &str) -> PathBuf {
        self.cache_dir.join(format!(
            "{}_exchange_balance.parquet",
            symbol.to_lowercase()
        ))
    }

    pub async fn fetch_total_balance(
        &self,
        symbol: &str,
        force_refresh: bool,
    ) -> Result<DataFrame> {
        let cache_path = self.cache_path(symbol);
        if !force_refresh && cache_path.exists() {
            if let Ok(df) = self.load_cache(&cache_path) {
                return Ok(df);
            }
        }

        let mut req = self
            .client
            .get(COINGLASS_BALANCE_URL)
            .header("CG-API-KEY", &self.api_key);
        req = req.query(&[("symbol", symbol)]);

        let resp = req
            .send()
            .await
            .context("Failed to fetch CoinGlass exchange balance data")?;
        let body = resp
            .text()
            .await
            .context("Failed reading CoinGlass exchange balance body")?;
        let parsed: CoinGlassResponse = serde_json::from_str(&body).with_context(|| {
            format!("Failed to parse CoinGlass exchange balance response: {body}")
        })?;

        if parsed.code != "0" {
            bail!(
                "CoinGlass exchange balance request failed: {} ({})",
                parsed.msg,
                parsed.code
            );
        }

        let series = parsed.data.into_iter().next().ok_or_else(|| {
            anyhow!("CoinGlass exchange balance response contained no data series")
        })?;
        let points = flatten_series(series)?;
        let df = points_to_df(points)?;
        self.save_cache(&cache_path, &df)?;
        Ok(df)
    }

    fn load_cache(&self, path: &PathBuf) -> Result<DataFrame> {
        use polars::io::parquet::ParquetReader;
        use std::fs::File;
        let file = File::open(path)?;
        Ok(ParquetReader::new(file).finish()?)
    }

    fn save_cache(&self, path: &PathBuf, df: &DataFrame) -> Result<()> {
        use polars::io::parquet::ParquetWriter;
        use std::fs::{self, File};
        fs::create_dir_all(path.parent().unwrap_or(path))?;
        let file = File::create(path)?;
        ParquetWriter::new(file).finish(&mut df.clone())?;
        Ok(())
    }
}

fn flatten_series(series: CoinGlassSeries) -> Result<Vec<ExchangeBalancePoint>> {
    let len = series.time_list.len();
    if series.price_list.len() != len {
        bail!("CoinGlass exchange balance response has mismatched time/price lengths");
    }

    let mut points = Vec::with_capacity(len);
    for i in 0..len {
        let total_balance = series
            .data_map
            .values()
            .map(|vals| vals.get(i).copied().unwrap_or(0.0))
            .sum::<f64>();
        points.push(ExchangeBalancePoint {
            time_ms: series.time_list[i],
            price: series.price_list[i],
            total_balance,
        });
    }
    Ok(points)
}

fn points_to_df(points: Vec<ExchangeBalancePoint>) -> Result<DataFrame> {
    let mut time = Vec::with_capacity(points.len());
    let mut price = Vec::with_capacity(points.len());
    let mut total_balance = Vec::with_capacity(points.len());
    let mut reserve_change_1 = vec![0.0; points.len()];
    let mut reserve_change_7 = vec![0.0; points.len()];

    for p in &points {
        time.push(p.time_ms);
        price.push(p.price);
        total_balance.push(p.total_balance);
    }

    for i in 1..points.len() {
        let prev = total_balance[i - 1];
        if prev.abs() > f64::EPSILON {
            reserve_change_1[i] = total_balance[i] / prev - 1.0;
        }
    }
    for i in 7..points.len() {
        let prev = total_balance[i - 7];
        if prev.abs() > f64::EPSILON {
            reserve_change_7[i] = total_balance[i] / prev - 1.0;
        }
    }

    Ok(DataFrame::new(vec![
        Series::new("time", time).cast(&DataType::Datetime(TimeUnit::Milliseconds, None))?,
        Series::new("price", price),
        Series::new("total_exchange_balance", total_balance),
        Series::new("reserve_change_1", reserve_change_1),
        Series::new("reserve_change_7", reserve_change_7),
    ])?)
}
