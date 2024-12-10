use crate::error::KryptoError;

#[derive(Debug, Clone)]
pub struct PrecisionData {
    ticker: String,
    tick_size: f64,
    step_size: f64,
    tick_precision: usize,
    step_precision: usize,
}

impl PrecisionData {
    pub fn new(ticker: String, tick_size: f64, step_size: f64) -> Self {
        let tick_precision = tick_size.log10().abs() as usize;
        let step_precision = step_size.log10().abs() as usize;
        Self {
            ticker,
            tick_size,
            step_size,
            tick_precision,
            step_precision,
        }
    }

    pub fn get_ticker(&self) -> &str {
        &self.ticker
    }

    pub fn get_tick_size(&self) -> f64 {
        self.tick_size
    }

    pub fn get_step_size(&self) -> f64 {
        self.step_size
    }

    pub fn get_tick_precision(&self) -> usize {
        self.tick_precision
    }

    pub fn get_step_precision(&self) -> usize {
        self.step_precision
    }

    pub fn fmt_price_to_string(&self, price: f64) -> String {
        format!("{:.1$}", price, self.tick_precision)
    }

    pub fn fmt_quantity_to_string(&self, quantity: f64) -> String {
        format!("{:.1$}", quantity, self.step_precision)
    }

    pub fn fmt_price(&self, price: f64) -> Result<f64, KryptoError> {
        format!("{:.1$}", price, self.tick_precision)
            .parse::<f64>()
            .map_err(|e| KryptoError::ParseError(e.to_string()))
    }

    pub fn fmt_quantity(&self, quantity: f64) -> Result<f64, KryptoError> {
        format!("{:.1$}", quantity, self.step_precision)
            .parse::<f64>()
            .map_err(|e| KryptoError::ParseError(e.to_string()))
    }
}
