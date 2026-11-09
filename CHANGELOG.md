# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
uses [Semantic Versioning](https://semver.org).

## [Unreleased]

### Added

- Cargo workspace with the `ripplefix-core`, `ripplefix-python`, and
  `ripplefix-cli` crates.
- `ripplefix analyze`: Git-based detection of structural Pydantic model
  changes, with evidence, confidence levels, wire-contract impact, JSON
  output, and `--fail-on-breaking`.
- `ripplefix doctor`: environment and plugin compatibility checks.
- Versioned analyzer plugin interface (API v1).
- `examples/fastapi-pydantic-ts` fixture and end-to-end tests.
- CI on Linux, Windows, and macOS, plus an MSRV check.
- `ripplefix impact`: traces breaking field changes to subclass models
  (across files), FastAPI routes, TypeScript types and usages, and Python
  tests, with an exportable evidence graph.
- `ripplefix-typescript` and `ripplefix-impact` crates.
- Supply-chain security: cargo-deny policy, CodeQL, Dependabot, SHA-pinned
  actions, CODEOWNERS, and `docs/compliance.md` (standards alignment).

### Fixed

- `Field(alias=...)` arguments no longer count as part of a field default,
  so an alias-preserving rename is correctly recognized as non-breaking.
