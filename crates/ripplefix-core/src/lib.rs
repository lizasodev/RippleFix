//! RippleFix core: domain model, structural change detection, Git integration,
//! and the versioned analyzer plugin interface.
//!
//! Everything in this crate is deterministic. Results always carry
//! [`Evidence`] and a [`Confidence`] level so that inferred relationships are
//! never presented as verified.

pub mod analysis;
pub mod diff;
pub mod model;
pub mod plugin;
pub mod vcs;

pub use analysis::{AnalysisReport, AnalyzeError, FileReport, FileStatus, analyze_worktree};
pub use diff::{Change, ChangeKind, diff_snapshots};
pub use model::{Confidence, DataModel, Evidence, Field, FileSnapshot, ModelKind, Span};
pub use plugin::{AnalysisError, LanguageAnalyzer, PLUGIN_API_VERSION};
