//! Git integration through the `git` executable.
//!
//! We invoke the system `git` (no shell, explicit argv) rather than linking
//! libgit2: it honors the user's Git version, worktrees, and sparse checkouts,
//! and adds no native build dependency. Repository-controlled settings that
//! could execute programs (`core.fsmonitor`, external diff, textconv) are
//! disabled for every invocation.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("could not run git (is it installed and on PATH?): {0}")]
    Spawn(#[from] std::io::Error),
    #[error("`git {args}` failed: {stderr}")]
    Failed { args: String, stderr: String },
    #[error("invalid revision `{0}`")]
    InvalidRevision(String),
    #[error("git produced non UTF-8 output for {0}")]
    Encoding(String),
}

/// Kind of change reported by `git diff --name-status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeltaStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Untracked,
}

/// One changed path between a base revision and the working tree.
/// Paths are repository-relative with forward slashes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FileDelta {
    pub status: DeltaStatus,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Repository {
    root: PathBuf,
}

impl Repository {
    /// Locate the repository containing `path`.
    pub fn discover(path: &Path) -> Result<Self, GitError> {
        let out = run_git(path, &["rev-parse", "--show-toplevel"])?;
        let root = String::from_utf8(out).map_err(|_| GitError::Encoding("toplevel".into()))?;
        Ok(Self {
            root: PathBuf::from(root.trim_end()),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn git(&self, args: &[&str]) -> Result<Vec<u8>, GitError> {
        run_git(&self.root, args)
    }

    /// Resolve `rev` to a full commit id.
    pub fn resolve_commit(&self, rev: &str) -> Result<String, GitError> {
        if rev.is_empty() || rev.starts_with('-') || rev.contains('\0') {
            return Err(GitError::InvalidRevision(rev.into()));
        }
        let spec = format!("{rev}^{{commit}}");
        let out = self
            .git(&[
                "rev-parse",
                "--verify",
                "--quiet",
                "--end-of-options",
                &spec,
            ])
            .map_err(|_| GitError::InvalidRevision(rev.into()))?;
        let sha = String::from_utf8(out).map_err(|_| GitError::Encoding(rev.into()))?;
        Ok(sha.trim().to_owned())
    }

    /// Paths changed between `commit` and the working tree (staged, unstaged,
    /// and untracked-but-not-ignored files).
    pub fn changed_files(&self, commit: &str) -> Result<Vec<FileDelta>, GitError> {
        let out = self.git(&[
            "diff",
            "--name-status",
            "-z",
            "-M",
            "--no-ext-diff",
            "--no-textconv",
            commit,
            "--",
        ])?;
        let mut deltas = parse_name_status(&out)?;
        let untracked = self.git(&["ls-files", "--others", "--exclude-standard", "-z"])?;
        for p in split_z(&untracked) {
            deltas.push(FileDelta {
                status: DeltaStatus::Untracked,
                old_path: None,
                new_path: Some(utf8(p)?),
            });
        }
        Ok(deltas)
    }

    /// Contents of `path` at `commit`, or `None` if it is not valid UTF-8 text.
    pub fn read_at(&self, commit: &str, path: &str) -> Result<Option<String>, GitError> {
        let out = self.git(&["show", "--no-textconv", &format!("{commit}:{path}")])?;
        Ok(String::from_utf8(out).ok())
    }

    /// Contents of `path` in the working tree, or `None` if missing / not UTF-8.
    pub fn read_worktree(&self, path: &str) -> Option<String> {
        std::fs::read(self.root.join(path))
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
    }

    /// Whether the working tree has staged, unstaged, or untracked changes.
    pub fn is_dirty(&self) -> Result<bool, GitError> {
        Ok(!self.git(&["status", "--porcelain", "-z"])?.is_empty())
    }
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<Vec<u8>, GitError> {
    tracing::debug!(?args, cwd = %cwd.display(), "running git");
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["-c", "core.fsmonitor=false", "-c", "core.quotepath=false"])
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !out.status.success() {
        return Err(GitError::Failed {
            args: args.join(" "),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        });
    }
    Ok(out.stdout)
}

fn split_z(buf: &[u8]) -> impl Iterator<Item = &[u8]> {
    buf.split(|b| *b == 0).filter(|s| !s.is_empty())
}

fn utf8(b: &[u8]) -> Result<String, GitError> {
    String::from_utf8(b.to_vec()).map_err(|_| GitError::Encoding(String::from_utf8_lossy(b).into()))
}

fn parse_name_status(buf: &[u8]) -> Result<Vec<FileDelta>, GitError> {
    let mut it = split_z(buf);
    let mut out = Vec::new();
    while let Some(status) = it.next() {
        let code = status.first().copied().unwrap_or(b'?');
        let mut next = || -> Result<String, GitError> {
            utf8(
                it.next()
                    .ok_or_else(|| GitError::Encoding("truncated name-status".into()))?,
            )
        };
        let delta = match code {
            b'A' => FileDelta {
                status: DeltaStatus::Added,
                old_path: None,
                new_path: Some(next()?),
            },
            b'D' => FileDelta {
                status: DeltaStatus::Deleted,
                old_path: Some(next()?),
                new_path: None,
            },
            b'R' | b'C' => {
                let old = next()?;
                let new = next()?;
                let status = if code == b'R' {
                    DeltaStatus::Renamed
                } else {
                    DeltaStatus::Copied
                };
                FileDelta {
                    status,
                    old_path: Some(old),
                    new_path: Some(new),
                }
            }
            b'T' => {
                let p = next()?;
                FileDelta {
                    status: DeltaStatus::TypeChanged,
                    old_path: Some(p.clone()),
                    new_path: Some(p),
                }
            }
            _ => {
                let p = next()?;
                FileDelta {
                    status: DeltaStatus::Modified,
                    old_path: Some(p.clone()),
                    new_path: Some(p),
                }
            }
        };
        out.push(delta);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_name_status_with_renames() {
        let raw = b"M\0a.py\0R092\0old.py\0new.py\0D\0gone.py\0A\0new file.py\0";
        let d = parse_name_status(raw).unwrap();
        assert_eq!(d.len(), 4);
        assert_eq!(d[1].status, DeltaStatus::Renamed);
        assert_eq!(d[1].old_path.as_deref(), Some("old.py"));
        assert_eq!(d[1].new_path.as_deref(), Some("new.py"));
        assert_eq!(d[2].new_path, None);
        assert_eq!(d[3].new_path.as_deref(), Some("new file.py"));
    }

    #[test]
    fn rejects_option_like_revisions() {
        let repo = Repository {
            root: PathBuf::from("."),
        };
        assert!(matches!(
            repo.resolve_commit("--output=x"),
            Err(GitError::InvalidRevision(_))
        ));
        assert!(matches!(
            repo.resolve_commit(""),
            Err(GitError::InvalidRevision(_))
        ));
    }
}
