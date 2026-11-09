//! Versioned interface implemented by language / framework analyzers.

use crate::model::FileSnapshot;
use std::path::Path;

/// Bumped on any breaking change to [`LanguageAnalyzer`] or the model types.
pub const PLUGIN_API_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AnalysisError {
    /// The source contains syntax errors. RippleFix refuses to diff it rather
    /// than risk reporting changes derived from a partial parse.
    #[error("syntax errors near line(s) {lines:?}; file was not analyzed")]
    SyntaxError { lines: Vec<usize> },
    #[error("parser failure: {0}")]
    Parser(String),
}

/// A deterministic analyzer for one language or framework.
pub trait LanguageAnalyzer: Send + Sync {
    /// Stable identifier, e.g. `python-pydantic`.
    fn id(&self) -> &'static str;
    /// Must equal [`PLUGIN_API_VERSION`] for the analyzer to be loaded.
    fn api_version(&self) -> u32;
    /// Whether this analyzer should process the given repository-relative path.
    fn handles(&self, path: &Path) -> bool;
    /// Extract the supported structures from one version of a file.
    fn extract(&self, source: &str) -> Result<FileSnapshot, AnalysisError>;
}
