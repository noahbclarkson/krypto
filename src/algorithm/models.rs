
use crate::algorithm::pls::{get_pls, predict as pls_predict};
use crate::error::KryptoError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ModelType {
    Pls,
}

pub fn train_and_predict(
    model_type: ModelType,
    train_features: &[Vec<f64>],
    train_labels: &[f64],
    test_features: &[Vec<f64>],
    n_components: usize,
) -> Result<Vec<f64>, KryptoError> {
    match model_type {
        ModelType::Pls => {
            let model = get_pls(train_features, train_labels, n_components)?;
            pls_predict(&model, test_features)
        }
    }
}
