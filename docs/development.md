# Development guide

## Setup

1. Install Rust with [rustup](https://rustup.rs). `rust-toolchain.toml`
   selects stable with clippy and rustfmt.
2. Install a C compiler for Tree-sitter: MSVC Build Tools on Windows,
   Xcode CLT on macOS, or `build-essential` on Linux.
3. Install Git 2.30 or later.

## Everyday commands

```sh
cargo build                                    # debug build
cargo run -p ripplefix-cli -- analyze -C path/to/repo
cargo test --workspace                         # unit and end-to-end tests
cargo fmt --all                                # format
cargo clippy --workspace --all-targets -- -D warnings
RIPPLEFIX_LOG=debug cargo run -p ripplefix-cli -- analyze   # structured logs on stderr
```

## Tests

* **Unit tests** live next to the code (`#[cfg(test)]`). The rules for diffs,
  Git output parsing, and Pydantic extraction are tested here.
* **End-to-end tests** (`crates/ripplefix-cli/tests/analyze.rs`) copy
  `examples/fastapi-pydantic-ts` into a temporary Git repository, edit it, run
  the real binary, and check its JSON output.
* To add a scenario, prefer a new end-to-end test that edits the fixture over
  adding mocked unit tests.

## Adding an analyzer

Implement `ripplefix_core::plugin::LanguageAnalyzer`, return
`PLUGIN_API_VERSION` from `api_version()`, and register it in
`crates/ripplefix-cli/src/main.rs::analyzers`. Analyzers must be pure: they
receive source text and must not read the file system or spawn processes.
