use getset::Getters;
use r_matrix::{matricies::RMatrix, data::RData};

use crate::BinanceDataId;


#[derive(Getters)]
#[getset(get = "pub")]
pub struct BinanceRMatrix {
    matricies: Box<[Option<Box<dyn RMatrix<BinanceDataId>>>]>,
    data: Box<[RData<BinanceDataId>]>,
}

impl BinanceRMatrix {
    pub fn new(matricies: Vec<Option<Box<dyn RMatrix<BinanceDataId>>>>, data: Vec<RData<BinanceDataId>>) -> Self {
        Self {
            matricies: matricies.into_boxed_slice(),
            data: data.into_boxed_slice(),
        }
    }

    pub fn predict(&self, index: usize) -> Vec<f64> {
        let mut predictions = Vec::with_capacity(self.matricies.len());
        for (matrix, data) in self.matricies.iter().zip(self.data.iter()) {
            if matrix.is_none() {
                predictions.push(0.0);
                continue;
            }
            let prediction = matrix.as_ref().unwrap().predict(data, index);
            predictions.push(prediction);
        }
        predictions
    }
}
