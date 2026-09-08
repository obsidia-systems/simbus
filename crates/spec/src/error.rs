//! Specification errors.

use std::path::PathBuf;

/// Errors produced while loading or validating YAML.
#[derive(Debug, thiserror::Error)]
pub enum SpecError {
    /// Filesystem failure.
    #[error("failed to read {path}: {source}")]
    Io {
        /// Path that could not be read.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// YAML did not match the schema.
    #[error("invalid YAML: {0}")]
    Yaml(#[from] serde_yaml::Error),
    /// Cross-references or value constraints failed.
    #[error("{0}")]
    Validation(String),
}
