<p align="center">
  <img src="docs/assets/banner.svg" alt="RippleFix: change code once, handle the consequences." width="100%">
</p>

<p align="center">

[![CI](https://github.com/lizasodev/RippleFix/actions/workflows/ci.yml/badge.svg)](https://github.com/lizasodev/RippleFix/actions/workflows/ci.yml)
[![CodeQL](https://github.com/lizasodev/RippleFix/actions/workflows/codeql.yml/badge.svg)](https://github.com/lizasodev/RippleFix/actions/workflows/codeql.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Good first issues](https://img.shields.io/github/issues/lizasodev/RippleFix/good%20first%20issue?label=good%20first%20issues&color=7057ff)](https://github.com/lizasodev/RippleFix/labels/good%20first%20issue)
[![Open in GitHub Codespaces](https://img.shields.io/badge/Codespaces-open-2dd4bf?logo=github)](https://codespaces.new/lizasodev/RippleFix)

</p>

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
git clone https://github.com/lizasodev/RippleFix
cd ripplefix
cargo install --locked --path crates/ripplefix-cli
ripplefix doctor
```

## Contributing

RippleFix is early, so this is a great time to shape it. You can be running
the tests in about 5 minutes, either locally or in
[Codespaces](https://codespaces.new/lizasodev/RippleFix).

* Pick a [good first issue](https://github.com/lizasodev/RippleFix/labels/good%20first%20issue)
  or a [help wanted](https://github.com/lizasodev/RippleFix/labels/help%20wanted) one.
  Each issue says which files to touch and what "done" means.
* Ask questions or propose ideas in [Discussions](https://github.com/lizasodev/RippleFix/discussions).
* Read the [contributing guide](CONTRIBUTING.md). It covers the three
  rules: deterministic only, never overstate certainty, and never run user
  code without consent.

## Documentation

* [Architecture](docs/architecture.md), including the [security model](docs/architecture.md#security-model)
* [Roadmap and status](docs/roadmap.md)
* [Standards alignment (ISO/IEC 27001, NIST SSDF, ISO/IEC 25010)](docs/compliance.md)
* [Development guide](docs/development.md)
* [Contributing](CONTRIBUTING.md) · [Code of Conduct](CODE_OF_CONDUCT.md) · [Security policy](SECURITY.md) · [Changelog](CHANGELOG.md)

## License

[Apache-2.0](LICENSE)
