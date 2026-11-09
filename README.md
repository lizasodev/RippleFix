# RippleFix

**Change code once. Handle the consequences.**

RippleFix detects the consequences of source-code changes across a repository
and automates the follow-up work using deterministic static analysis. It runs
locally, explains every result with evidence, and contains no AI.

> **Status: early development (0.1.0, unreleased).** Today RippleFix detects
> structural changes to Pydantic models between a Git revision and your
> working tree. Impact tracing, patch generation, and the editor and dashboard
> interfaces have not been built yet. See [docs/roadmap.md](docs/roadmap.md)
> for exact status.

## What works today

```console
$ ripplefix analyze
RippleFix analysis against 0de79e156d09

  backend/app/models.py
    UserBase: field renamed: full_name -> display_name (INFERRED) [breaking]
        evidence: field `full_name` removed backend/app/models.py:8
        evidence: field `display_name` added at the same position with identical type `str` and default backend/app/models.py:8
    UserRead: field default changed: is_active: True -> <required> (confirmed) [breaking]
        evidence: default value changed backend/app/models.py:18

2 structural change(s), 2 affecting the wire contract, 1 not fully confirmed. 0 changed file(s) not supported by any analyzer.
```

* Detects model, field, type, default, base-class, and alias (JSON key)
  changes in Pydantic models.
* Labels each finding `confirmed`, `inferred`, or `unresolved`, so heuristics
  are never presented as facts.
* Treats formatting and comment edits as cosmetic.
* Refuses to analyze files with syntax errors instead of guessing.
* Prints JSON with `--format json` and fails CI with `--fail-on-breaking`
  (exit code 1).

## Install from source

Requirements: Rust 1.85 or later, Git, and a C compiler (MSVC Build Tools on
Windows) for the Tree-sitter grammars.

```sh
cargo install --path crates/ripplefix-cli
ripplefix doctor
ripplefix analyze --base origin/main
```

## Documentation

* [Architecture](docs/architecture.md)
* [Roadmap and status](docs/roadmap.md)
* [Development guide](docs/development.md)
* [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [Changelog](CHANGELOG.md)

## License

Apache-2.0
