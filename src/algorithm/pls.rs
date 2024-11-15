use linfa::traits::Fit;
use linfa_pls::PlsRegression;
use ndarray::Array2;

use crate::error::KryptoError;

pub fn get_pls(
    predictors: Vec<Vec<f64>>,
    target: Vec<f64>,
    n: usize,
) -> Result<PlsRegression<f64>, KryptoError> {
    let flattened_predictors: Vec<f64> = predictors.iter().flatten().copied().collect();
    let predictors: Array2<f64> = Array2::from_shape_vec(
        (predictors.len(), predictors[0].len()),
        flattened_predictors,
    )
    .unwrap();
    let target: Array2<f64> = Array2::from_shape_vec((target.len(), 1), target).unwrap();
    let ds = linfa::dataset::Dataset::new(predictors, target);
    let pls = PlsRegression::params(n)
        .fit(&ds)
        .map_err(|e| KryptoError::FitError(e.to_string()))?;
    Ok(pls)
}
