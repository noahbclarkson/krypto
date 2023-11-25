use std::f64::consts::PI;

use serde::{Deserialize, Serialize};
use statrs::function::erf::erf;

#[inline]
pub(crate) fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

#[inline]
pub(crate) fn standard_deviation(values: &[f64]) -> f64 {
    let mean = mean(values);
    let variance = values
        .iter()
        .map(|value| (value - mean) * (value - mean))
        .sum::<f64>()
        / values.len() as f64;
    variance.sqrt()
}

#[inline]
pub(crate) fn probability_positive(values: &[f64]) -> f64 {
    let mut positive = 0.0;
    for value in values {
        if *value > 0.0 {
            positive += 1.0;
        } else if *value == 0.0 {
            positive += 0.5;
        }
    }
    positive / values.len() as f64
}

#[inline]
pub fn norm_s_dist(z_score: f64) -> f64 {
    0.5 * (1.0 + erf(z_score / (2.0f64).sqrt()))
}

#[inline(always)]
pub fn bayes_combine(prior: f64, likelihood: f64) -> f64 {
    (prior * likelihood) / ((prior * likelihood) + ((1.0 - prior) * (1.0 - likelihood)))
}

#[inline]
pub fn format_number(num: f64) -> String {
    let original_num = num;
    let suffixes = [
        "", "K", "M", "B", "T", "Qa", "Qi", "Sx", "Sp", "Oc", "No", "De", "Ud", "Dd", "Td", "Qad",
        "Qid", "Sxd", "Spd", "Od", "Nd", "V", "Ct",
    ];

    let mut num = num;
    let mut index = 0;

    while num >= 1000.0 && index < suffixes.len() - 1 {
        num /= 1000.0;
        index += 1;
    }

    match original_num {
        n if n < 10.0 => format!("{:.4}{}", num, suffixes[index]),
        n if n < 100.0 => format!("{:.3}{}", num, suffixes[index]),
        _ => format!("{:.2}{}", num, suffixes[index]),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
