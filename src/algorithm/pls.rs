use linfa::{
    prelude::AsTargets as _,
    traits::{Fit, Predict as _},
};
use linfa_pls::PlsRegression;
use ndarray::Array2;
use tracing::{debug, error, warn};

use crate::error::KryptoError;

/// Fits a PLS regression model.
///
/// # Arguments
/// * `predictors` - Feature matrix (rows: samples, columns: features).
/// * `target` - Target vector (column vector).
/// * `n` - Number of PLS components to extract.
///
/// # Returns
/// A trained `PlsRegression` model or a `KryptoError`.
pub fn get_pls(
    predictors: &[Vec<f64>], // Use slice for flexibility
    target: &[f64],          // Use slice
    n: usize,
) -> Result<PlsRegression<f64>, KryptoError> {
    if predictors.is_empty() || target.is_empty() {
        return Err(KryptoError::InsufficientData {
            got: predictors.len(),
            required: 1,
            context: "Predictors or target vector is empty for PLS fitting".to_string(),
        });
    }
    if predictors.len() != target.len() {
        return Err(KryptoError::InvalidDatasetLengths(
            predictors.len(),
            target.len(),
            0,
        )); // 0 for candles as not relevant here
    }

    let n_samples = predictors.len();
    let n_features = predictors[0].len();

    if n_features == 0 {
        return Err(KryptoError::InsufficientData {
            got: 0,
            required: 1,
            context: "No features provided for PLS fitting".to_string(),
        });
    }

    // Validate n (number of components)
    if n == 0 {
        return Err(KryptoError::ConfigError(
            "Number of PLS components (n) must be greater than 0".to_string(),
        ));
    }
    // PLS components cannot exceed the number of features or samples (whichever is smaller)
    let max_components = n_samples.min(n_features);
    if n > max_components {
        warn!(
            "Requested PLS components ({}) exceeds max possible ({}), reducing to max.",
            n, max_components
        );
        // n = max_components; // Adjust n or return error? Adjusting might hide issues. Let linfa handle it?
        // Let's return an error for clarity. The GA should ideally not generate invalid n.
        return Err(KryptoError::ConfigError(format!(
            "Number of PLS components ({}) cannot exceed min(samples, features) ({})",
            n, max_components
        )));
    }

    // Flatten predictors safely
    let flattened_predictors: Vec<f64> = predictors.iter().flatten().copied().collect();
    let predictors_array = Array2::from_shape_vec((n_samples, n_features), flattened_predictors)?;

    // Check for NaN/Infinity in predictors *before* fitting
    if predictors_array
        .iter()
        .any(|&x| x.is_nan() || x.is_infinite())
    {
        error!("NaN or Infinity detected in predictor data before PLS fit.");
        // Consider more detailed logging here about *where* the invalid data is.
        return Err(KryptoError::PlsInternalError(
            "Invalid data (NaN/Inf) in predictors.".to_string(),
        ));
    }

    // Shape target correctly
    let target_array = Array2::from_shape_vec((target.len(), 1), target.to_vec())?; // Clone target into Vec

    // Check for NaN/Infinity in target *before* fitting
    if target_array.iter().any(|&x| x.is_nan() || x.is_infinite()) {
        error!("NaN or Infinity detected in target data before PLS fit.");
        return Err(KryptoError::PlsInternalError(
            "Invalid data (NaN/Inf) in target.".to_string(),
        ));
    }

    let dataset = linfa::dataset::Dataset::new(predictors_array, target_array);

    debug!(
        "Fitting PLS model with {} components, {} samples, {} features",
        n, n_samples, n_features
    );

    // Directly call fit and propagate the error using ?
    // No need for catch_unwind if the underlying library returns Result
    PlsRegression::params(n)
        .fit(&dataset)
        .map_err(KryptoError::PlsFitError) // Map linfa error to KryptoError::PlsFitError
}

/// Makes predictions using a trained PLS model.
///
/// # Arguments
/// * `pls` - The trained `PlsRegression` model.
/// * `features` - The feature matrix to predict on.
///
/// # Returns
/// A `Vec<f64>` of predictions or a `KryptoError`.
pub fn predict(pls: &PlsRegression<f64>, features: &[Vec<f64>]) -> Result<Vec<f64>, KryptoError> {
    if features.is_empty() {
        return Ok(Vec::new()); // No features to predict
    }

    let n_samples = features.len();
    let n_features = features[0].len(); // Assume consistent feature length

    if n_features == 0 {
        return Err(KryptoError::InsufficientData {
            got: 0,
            required: 1,
            context: "No features provided for PLS prediction".to_string(),
        });
    }

    // Check model compatibility (optional but good practice)
    // if n_features != pls.n_features() { ... return error ... }

    let flat_features: Vec<f64> = features.iter().flatten().cloned().collect();
    let arr_features = Array2::from_shape_vec((n_samples, n_features), flat_features)?;

    // Check for NaN/Infinity in features *before* predicting
    if arr_features.iter().any(|&x| x.is_nan() || x.is_infinite()) {
        error!("NaN or Infinity detected in feature data before PLS predict.");
        // Handle this - maybe return default predictions or error out? Erroring is safer.
        return Err(KryptoError::PlsInternalError(
            "Invalid data (NaN/Inf) in features for prediction.".to_string(),
        ));
    }

    // Predict and convert result
    let y_hat = pls.predict(&arr_features).as_targets().to_owned();
    Ok(y_hat.into_raw_vec())
}
