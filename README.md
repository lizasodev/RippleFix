# RippleFix

**Change code once. Handle the consequences.**

[![CI](https://github.com/lizasodev/ripplefix/actions/workflows/ci.yml/badge.svg)](https://github.com/lizasodev/ripplefix/actions/workflows/ci.yml)
[![CodeQL](https://github.com/lizasodev/ripplefix/actions/workflows/codeql.yml/badge.svg)](https://github.com/lizasodev/ripplefix/actions/workflows/codeql.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

When you rename a field in a Pydantic model, the API changes, and so do the
TypeScript types, the frontend code, and the tests that depend on it.
RippleFix finds those consequences for you, shows the evidence for each one,
and (soon) generates safe, reviewable patches.

* **Deterministic.** Static analysis with Tree-sitter, import resolution,
  and explicit rules. No AI, no models, no guesses.
* **Honest.** Every finding is labeled `confirmed`, `inferred`, or
  `unresolved`, with file and line evidence. Heuristics are never presented
  as facts.
* **Local and private.** No network access and no telemetry. Your code never
  leaves your machine.
* **Safe.** It never runs repository code or modifies files without your
  explicit consent.

> **Status: early development (0.1.0, unreleased).** Change detection and
> impact analysis work. Patch generation, validation, and the VS Code and
> dashboard interfaces are not built yet. See [docs/roadmap.md](docs/roadmap.md).

## Example

In [`examples/fastapi-pydantic-ts`](examples/fastapi-pydantic-ts), rename
`full_name` to `display_name` in `UserBase`:

```console
$ ripplefix impact
RippleFix impact (4 files indexed)

backend/app/models.py  UserBase: field renamed: full_name -> display_name (INFERRED) [breaking]
    [ confirmed] Model           UserCreate  backend/app/models.py:11
    [ confirmed] Model           UserRead  backend/app/models.py:15
    [ confirmed] Route           POST /users (create_user)  backend/app/main.py:11
                   evidence: route uses `UserRead` as its response type, which resolves to model `UserRead` backend/app/main.py:11
                   evidence: `UserRead` imported here backend/app/main.py:5
    [ confirmed] Route           GET /users/{user_id} (get_user)  backend/app/main.py:23
    [  INFERRED] TypescriptType  UserRead  frontend/src/api.ts:1
                   evidence: all 5 properties of `UserRead` are JSON keys of `UserRead` before the change, including `full_name`
    [  INFERRED] TypescriptUsage full_name (propertyaccess)  frontend/src/api.ts:16
    [  INFERRED] Test            backend/tests/test_users.py:9
                   evidence: test calls POST /users backend/tests/test_users.py:9
                   evidence: string literal "full_name" backend/tests/test_users.py:11
```

(Abridged. Every item prints its full evidence chain.)

## Commands

| Command | Purpose |
|---|---|
| `ripplefix analyze [--base REV]` | Structural changes to Pydantic models between `REV` (default `HEAD`) and the working tree. Formatting and comment edits are reported as cosmetic |
| `ripplefix impact [--base REV]` | Traces breaking field changes to subclass models, FastAPI routes, TypeScript types and usages, and tests |
| `ripplefix doctor` | Checks Git, the repository, and plugin compatibility |

Common options: `--format json` (stable machine output, including the impact
graph's nodes and edges), `-C <path>` to target another repository, and
`-v`/`-vv` or `RIPPLEFIX_LOG=debug` for structured logs.
`analyze --fail-on-breaking` exits with status 1 for use in CI.

### What is detected

* **Models:** classes inheriting `pydantic.BaseModel`, directly or through
  other models, including across files via `import` resolution.
* **Changes:** models or fields added or removed, renames (inferred by a
  strict rule), type, default, and base-class changes, and JSON-key changes
  through `alias` / `serialization_alias`.
* **Consumers:** FastAPI routes (`response_model`, return annotation, typed
  parameters), TypeScript `interface` / object `type` declarations that
  mirror the model, property accesses and object keys in TypeScript, and
  Python tests that call an affected route and use the old key.

## Install from source

Requirements: Rust 1.85 or later, Git, and a C compiler (MSVC Build Tools on
Windows, Xcode CLT on macOS, `build-essential` on Linux).

```sh
git clone https://github.com/lizasodev/ripplefix
cd ripplefix
cargo install --locked --path crates/ripplefix-cli
ripplefix doctor
```

## Documentation

* [Architecture](docs/architecture.md), including the [security model](docs/architecture.md#security-model)
* [Roadmap and status](docs/roadmap.md)
* [Standards alignment (ISO/IEC 27001, NIST SSDF, ISO/IEC 25010)](docs/compliance.md)
* [Development guide](docs/development.md)
* [Contributing](CONTRIBUTING.md) · [Security policy](SECURITY.md) · [Changelog](CHANGELOG.md)

## License

[Apache-2.0](LICENSE)
