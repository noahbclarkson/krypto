pub mod dataset;
pub mod error;
pub mod normalization_function;
pub mod r_matrix;
pub mod math;

pub use dataset::{DataPoint, Dataset, Features, Labels};
pub use error::RMatrixError;
pub use normalization_function::NormalizationFunctionType;
pub use r_matrix::matrix::RMatrix;
