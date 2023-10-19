
#[derive(Debug, thiserror::Error)]
pub enum ConfigurationError {
    #[error("Unable to find configuration file.")]
    FileNotFound,
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("YAML error: {0}")]
    YamlError(#[from] serde_yaml::Error),
}