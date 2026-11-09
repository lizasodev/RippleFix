# Contributing to RippleFix

Thanks for being here! RippleFix is young, so your contribution has a real
impact on what it becomes. Every kind of help counts: code, bug reports with
a small reproduction, docs, fixtures from real projects, or design feedback.

## Start in 5 minutes

**Option A, in the browser:** open the repository in GitHub Codespaces
(**Code → Codespaces → Create**). The dev container installs Rust and runs
the first build for you.

**Option B, locally:** you need Rust (via [rustup](https://rustup.rs)), Git,
and a C compiler (MSVC Build Tools on Windows, Xcode CLT on macOS,
`build-essential` on Linux).

```sh
git clone https://github.com/lizasodev/RippleFix
cd RippleFix
cargo test --workspace      # all tests should pass
cargo run -q -p ripplefix-cli -- doctor
```

To see RippleFix work on the example project:

```sh
cp -r examples/fastapi-pydantic-ts /tmp/demo && cd /tmp/demo
git init -q && git add . && git commit -qm init
sed -i 's/full_name: str/display_name: str/' backend/app/models.py
cargo run -q --manifest-path ~/RippleFix/Cargo.toml -p ripplefix-cli -- impact
```

## Find something to work on

| If you want... | Look at |
|---|---|
| A small first task | [`good first issue`](https://github.com/lizasodev/RippleFix/labels/good%20first%20issue) |
| A meatier problem | [`help wanted`](https://github.com/lizasodev/RippleFix/labels/help%20wanted) |
| To shape the next big feature | [`epic`](https://github.com/lizasodev/RippleFix/labels/epic) issues and [Discussions](https://github.com/lizasodev/RippleFix/discussions) |

Comment on an issue before you start so nobody duplicates work. If an issue
has had no activity from its assignee for 2 weeks, feel free to ask to take
it over.

## How the code is organized

| Crate | What lives there | Good to touch for... |
|---|---|---|
| `ripplefix-core` | Data model, structural diff, Git layer, plugin trait | Change detection rules |
| `ripplefix-python` | Tree-sitter extraction of Pydantic models, imports, FastAPI routes | New Python patterns |
| `ripplefix-typescript` | Tree-sitter extraction of TS types and usages | New TS patterns |
| `ripplefix-impact` | Project index, import resolution, impact rules, graph | New relationships |
| `ripplefix-cli` | The `ripplefix` binary, rendering, exit codes | UX and output |

[docs/architecture.md](docs/architecture.md) explains the design, and
[docs/roadmap.md](docs/roadmap.md) shows what is done and what is next.

## The three rules of RippleFix

1. **Deterministic only.** No AI, language models, embeddings, or network
   services. Pull requests that add them will be closed.
2. **Never overstate certainty.** If the syntax doesn't prove a fact, report
   it as `inferred` or `unresolved`, with evidence. A missed result is better
   than a false one.
3. **Never run or modify user code without consent.** RippleFix analyzes
   untrusted repositories (see [SECURITY.md](SECURITY.md)).

## Making a change

1. Fork the repository and create a branch, for example
   `feat/router-prefix`.
2. Write tests. The best ones edit the fixture in `examples/` and run the
   real binary (see `crates/ripplefix-cli/tests/analyze.rs`).
3. Run the same checks as CI:
   ```sh
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
4. Use a [Conventional Commit](https://www.conventionalcommits.org) title,
   for example `feat(python): resolve router prefixes`.
5. Add a line to `CHANGELOG.md` under *Unreleased*, and update docs if
   behavior changes.
6. Open the pull request. CI runs on Linux, Windows, and macOS, plus a
   supply-chain audit and CodeQL. All checks must pass before merging.

New dependencies need a short justification in the pull request, and their
license must be on the allowlist in `deny.toml`.

## Reporting bugs

Use the bug report template and include the smallest *before* and *after*
source that reproduces the problem, plus `--format json -vv` output. Report
security problems privately, as described in [SECURITY.md](SECURITY.md).

## Community

Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).
Questions and ideas are welcome in
[Discussions](https://github.com/lizasodev/RippleFix/discussions).

By contributing, you agree that your contributions are licensed under the
[Apache-2.0 license](LICENSE).
