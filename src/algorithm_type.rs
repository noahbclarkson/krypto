use core::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, PartialEq)]
pub enum AlgorithmType {
    RandomForest,
    PartialLeastSquares,
    RMatrix,
}

impl Serialize for AlgorithmType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            AlgorithmType::RandomForest => serializer.serialize_str("RandomForest"),
            AlgorithmType::PartialLeastSquares => serializer.serialize_str("PartialLeastSquares"),
            AlgorithmType::RMatrix => serializer.serialize_str("RMatrix"),
        }
    }
}

impl<'de> Deserialize<'de> for AlgorithmType {
    fn deserialize<D>(deserializer: D) -> Result<AlgorithmType, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "RandomForest" => Ok(AlgorithmType::RandomForest),
            "PartialLeastSquares" => Ok(AlgorithmType::PartialLeastSquares),
            "RMatrix" => Ok(AlgorithmType::RMatrix),
            _ => Err(serde::de::Error::custom("Unknown algorithm type")),
        }
    }
}

impl fmt::Display for AlgorithmType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AlgorithmType::RandomForest => write!(f, "RandomForest"),
            AlgorithmType::PartialLeastSquares => write!(f, "PartialLeastSquares"),
            AlgorithmType::RMatrix => write!(f, "RMatrix"),
        }
    }
}
