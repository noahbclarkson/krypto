pub mod dataset;
pub mod error;
pub mod math;
pub mod normalization_function;
pub mod r_matrix;
pub mod return_calculator;

pub use dataset::{DataPoint, Dataset, Features, Labels};
pub use error::RMatrixError;
pub use normalization_function::NormalizationFunctionType;
pub use r_matrix::matrix::RMatrix;
