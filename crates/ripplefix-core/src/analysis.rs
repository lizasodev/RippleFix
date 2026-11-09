//! Incremental change analysis: only files Git reports as changed are parsed.

use crate::diff::{Change, diff_snapshots};
use crate::model::FileSnapshot;
use crate::plugin::{LanguageAnalyzer, PLUGIN_API_VERSION};
use crate::vcs::{DeltaStatus, FileDelta, GitError, Repository};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum AnalyzeError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("analyzer `{id}` targets plugin API v{found}, but this build supports v{expected}")]
    IncompatibleAnalyzer {
        id: &'static str,
        found: u32,
        expected: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum FileStatus {
    /// At least one supported structure changed.
    Structural,
    /// The file changed but no supported structure did (formatting, comments,
    /// method bodies, unrelated code).
    Cosmetic,
    /// The file could not be analyzed; no conclusions are drawn from it.
    Unanalyzable { reason: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct FileReport {
    pub delta: FileDelta,
    pub analyzer: &'static str,
    #[serde(flatten)]
    pub status: FileStatus,
    pub changes: Vec<Change>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisReport {
    pub base_commit: String,
    /// Files handled by an analyzer, sorted by path.
    pub files: Vec<FileReport>,
    /// Number of changed files no analyzer supports.
    pub unsupported_files: usize,
}

impl AnalysisReport {
    pub fn changes(&self) -> impl Iterator<Item = &Change> {
        self.files.iter().flat_map(|f| &f.changes)
    }

    pub fn has_breaking_changes(&self) -> bool {
        self.changes().any(|c| c.breaks_wire_contract)
    }
}

/// Compare the working tree of `repo` against `base_rev` using `analyzers`.
pub fn analyze_worktree(
    repo: &Repository,
    base_rev: &str,
    analyzers: &[&dyn LanguageAnalyzer],
) -> Result<AnalysisReport, AnalyzeError> {
    for a in analyzers {
        if a.api_version() != PLUGIN_API_VERSION {
            return Err(AnalyzeError::IncompatibleAnalyzer {
                id: a.id(),
                found: a.api_version(),
                expected: PLUGIN_API_VERSION,
            });
        }
    }
    let base = repo.resolve_commit(base_rev)?;
    let mut deltas = repo.changed_files(&base)?;
    deltas.sort_by(|a, b| sort_key(a).cmp(sort_key(b)));

    let mut files = Vec::new();
    let mut unsupported = 0;
    for delta in deltas {
        let analyzer = [delta.new_path.as_deref(), delta.old_path.as_deref()]
            .into_iter()
            .flatten()
            .find_map(|p| analyzers.iter().find(|a| a.handles(Path::new(p))));
        let Some(analyzer) = analyzer else {
            unsupported += 1;
            continue;
        };
        files.push(analyze_delta(repo, &base, *analyzer, delta)?);
    }
    tracing::info!(files = files.len(), unsupported, "analysis complete");
    Ok(AnalysisReport {
        base_commit: base,
        files,
        unsupported_files: unsupported,
    })
}

fn sort_key(d: &FileDelta) -> &str {
    d.new_path
        .as_deref()
        .or(d.old_path.as_deref())
        .unwrap_or_default()
}

fn analyze_delta(
    repo: &Repository,
    base: &str,
    analyzer: &dyn LanguageAnalyzer,
    delta: FileDelta,
) -> Result<FileReport, AnalyzeError> {
    // A copy leaves the original in place, so the copy starts from nothing.
    let old_source = match (&delta.old_path, delta.status) {
        (Some(p), s) if s != DeltaStatus::Copied => Some(repo.read_at(base, p)?),
        _ => None,
    };
    let new_source = delta.new_path.as_deref().map(|p| repo.read_worktree(p));

    let report = |status, changes| FileReport {
        delta: delta.clone(),
        analyzer: analyzer.id(),
        status,
        changes,
    };
    let snapshot = |src: Option<Option<String>>, side: &str| match src {
        None => Ok(FileSnapshot::default()),
        Some(None) => Err(format!("{side} version is not readable UTF-8 text")),
        Some(Some(text)) => analyzer
            .extract(&text)
            .map_err(|e| format!("{side} version: {e}")),
    };

    let (old, new) = match (
        snapshot(old_source, "base"),
        snapshot(new_source, "working tree"),
    ) {
        (Ok(o), Ok(n)) => (o, n),
        (Err(reason), _) | (_, Err(reason)) => {
            return Ok(report(FileStatus::Unanalyzable { reason }, Vec::new()));
        }
    };
    let old_path = delta.old_path.as_deref().unwrap_or_default();
    let new_path = delta.new_path.as_deref().unwrap_or(old_path);
    let changes = diff_snapshots(&old, &new, old_path, new_path);
    let status = if changes.is_empty() {
        FileStatus::Cosmetic
    } else {
        FileStatus::Structural
    };
    Ok(report(status, changes))
}
