# ADR 0001: Use the git CLI instead of libgit2

Status: accepted (2026-11-09)

## Context

Change detection needs diffs between a revision and the working tree, a list
of untracked files, and file contents at a revision. The options were `git2`
(libgit2 bindings), `gix` (gitoxide), and the `git` executable.

## Decision

Use the `git` executable via `std::process::Command`.

* It behaves exactly like the user's Git: worktrees, sparse checkout,
  `.gitignore`, and rename detection all match.
* It adds no native C build dependency (libgit2/OpenSSL), which simplifies
  cross-platform builds.
* RippleFix needs only a few read-only commands.

## Consequences

* Git must be on `PATH`. `ripplefix doctor` checks for it.
* Every call costs a process spawn. Analysis spawns about 3 + N processes for
  N changed supported files, which is acceptable for interactive use. If
  benchmarks show a problem, we will switch to `git cat-file --batch`.
* Repository config can define programs that git runs. We disable
  `core.fsmonitor`, external diff, and textconv on every call (see
  `crates/ripplefix-core/src/vcs.rs`).
