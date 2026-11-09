# Contributing to RippleFix

Thanks for your interest! See [docs/development.md](docs/development.md) for
setup and commands.

## Ground rules

* **Deterministic only.** RippleFix must not use AI, language models,
  embeddings, or network services. Pull requests that add them will be closed.
* **Never overstate certainty.** If a fact cannot be proven from source, it
  must be reported as `inferred` or `unresolved`, with evidence.
* **Tests are required** for new behavior. Tests that copy the fixture and run
  the real binary are preferred.
* Keep dependencies minimal and explain each new one in the pull request.

## Workflow

1. Open an issue for any non-trivial change.
2. Use [Conventional Commits](https://www.conventionalcommits.org), for
   example `feat(python): resolve inherited fields across files`.
3. Make sure `cargo fmt --all --check`, `cargo clippy --workspace --all-targets
   -- -D warnings`, and `cargo test --workspace` pass.
4. Update `CHANGELOG.md` under *Unreleased*, and update the docs when
   behavior changes.

The project follows [Semantic Versioning](https://semver.org).
