use ta::{Open, Close, Low, High, Volume};

#[derive(Debug, PartialEq)]
pub struct Bar {
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

impl Bar {
    pub fn new() -> Self {
        Self {
            open: 0.0,
            close: 0.0,
            low: 0.0,
            high: 0.0,
            volume: 0.0,
        }
    }

    //pub fn open<T: Into<f64>>(mut self, val :T ) -> Self {
    //    self.open = val.into();
    //    self
    //}

    pub fn high<T: Into<f64>>(mut self, val: T) -> Self {
        self.high = val.into();
        self
    }

    pub fn low<T: Into<f64>>(mut self, val: T) -> Self {
        self.low = val.into();
        self
    }

    pub fn close<T: Into<f64>>(mut self, val: T) -> Self {
        self.close = val.into();
        self
    }

    pub fn volume(mut self, val: f64) -> Self {
        self.volume = val;
        self
    }
}

impl Open for Bar {
    fn open(&self) -> f64 {
        self.open
    }
}

impl Close for Bar {
    fn close(&self) -> f64 {
        self.close
    }
}

impl Low for Bar {
    fn low(&self) -> f64 {
        self.low
    }
}

impl High for Bar {
    fn high(&self) -> f64 {
        self.high
    }
}

impl Volume for Bar {
    fn volume(&self) -> f64 {
        self.volume
    }
}

pub fn round(num: f64) -> f64 {
    (num * 1000.0).round() / 1000.00
}