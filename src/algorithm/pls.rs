use linfa::traits::{Fit, Predict as _};
use linfa_pls::PlsRegression;
use ndarray::Array2;

use crate::error::KryptoError;

pub fn get_pls(
    predictors: Vec<Vec<f64>>,
    target: Vec<f64>,
    n: usize,
) -> Result<PlsRegression<f64>, KryptoError> {
    let flattened_predictors: Vec<f64> = predictors.iter().flatten().copied().collect();
    let shape = (predictors.len(), predictors[0].len());
    let predictors: Array2<f64> = Array2::from_shape_vec(shape, flattened_predictors)?;
    let target: Array2<f64> = Array2::from_shape_vec((target.len(), 1), target)?;
    let ds = linfa::dataset::Dataset::new(predictors, target);
    let pls = PlsRegression::params(n).fit(&ds)?;
    Ok(pls)
}

pub fn predict(pls: &PlsRegression<f64>, features: &[Vec<f64>]) -> Result<Vec<f64>, KryptoError> {
    let flat_features: Vec<f64> = features.iter().flatten().cloned().collect();
    let arr_features = Array2::from_shape_vec((features.len(), features[0].len()), flat_features)?;
    Ok(pls.predict(&arr_features).into_raw_vec())
}
