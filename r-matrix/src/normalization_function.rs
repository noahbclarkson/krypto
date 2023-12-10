use std::f64::consts::PI;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Default)]
/// A struct that represents a normalization function.
pub enum NormalizationFunctionType {
    #[default]
    Tanh,
    Gudermannian,
    AlgebraicSigmoid,
    Softsign,
}

impl NormalizationFunctionType {
    pub fn from_string(string: &str) -> Self {
        match string {
            "Tanh" => NormalizationFunctionType::Tanh,
            "Gudermannian" => NormalizationFunctionType::Gudermannian,
            "Algebraic Sigmoid" => NormalizationFunctionType::AlgebraicSigmoid,
            "Softsign" => NormalizationFunctionType::Softsign,
            _ => NormalizationFunctionType::Tanh,
        }
    }

    /// Get the normalization function.
    pub fn get_function(&self) -> fn(f64) -> f64 {
        match self {
            NormalizationFunctionType::Tanh => Self::tanh,
            NormalizationFunctionType::Gudermannian => Self::gudermannian,
            NormalizationFunctionType::AlgebraicSigmoid => Self::algebraic_sigmoid,
            NormalizationFunctionType::Softsign => Self::softsign,
        }
    }

    /// Get the derivative of the normalization function.
    pub fn get_derivative(&self) -> fn(f64) -> f64 {
        match self {
            NormalizationFunctionType::Tanh => Self::tanh_derivative,
            NormalizationFunctionType::Gudermannian => Self::gudermannian_derivative,
            NormalizationFunctionType::AlgebraicSigmoid => Self::algebraic_sigmoid_derivative,
            NormalizationFunctionType::Softsign => Self::softsign_derivative,
        }
    }

    /// Get the name of the normalization function.
    pub fn get_name(&self) -> &str {
        match self {
            NormalizationFunctionType::Tanh => "Tanh",
            NormalizationFunctionType::Gudermannian => "Gudermannian",
            NormalizationFunctionType::AlgebraicSigmoid => "Algebraic Sigmoid",
            NormalizationFunctionType::Softsign => "Softsign",
        }
    }

    pub fn from_index(index: usize) -> Self {
        match index {
            0 => NormalizationFunctionType::Tanh,
            1 => NormalizationFunctionType::Gudermannian,
            2 => NormalizationFunctionType::AlgebraicSigmoid,
            3 => NormalizationFunctionType::Softsign,
            _ => NormalizationFunctionType::Tanh,
        }
    }

    fn tanh(x: f64) -> f64 {
        x.tanh()
    }

    fn tanh_derivative(x: f64) -> f64 {
        let tanh = x.tanh();
        1.0 - tanh * tanh
    }

    fn gudermannian(x: f64) -> f64 {
        (2.0 / PI) * (x.atan() - (PI / 2.0))
    }

    fn gudermannian_derivative(x: f64) -> f64 {
        2.0 / (PI * (1.0 + x * x))
    }

    fn algebraic_sigmoid(x: f64) -> f64 {
        x / (1.0 + x.abs())
    }

    fn algebraic_sigmoid_derivative(x: f64) -> f64 {
        1.0 / ((1.0 + x.abs()) * (1.0 + x.abs()))
    }

    fn softsign(x: f64) -> f64 {
        x / (1.0 + x.abs())
    }

    fn softsign_derivative(x: f64) -> f64 {
        1.0 / ((1.0 + x.abs()) * (1.0 + x.abs()))
    }
}

impl Serialize for NormalizationFunctionType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.get_name())
    }
}

impl<'de> Deserialize<'de> for NormalizationFunctionType {
    fn deserialize<D>(deserializer: D) -> Result<NormalizationFunctionType, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(NormalizationFunctionType::from_string(&s))
    }
}
