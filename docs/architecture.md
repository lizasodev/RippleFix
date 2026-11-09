# RippleFix Architecture

RippleFix is a local-first, deterministic tool. Every result it reports is
derived from source syntax, Git history, or explicit rules, and comes with
evidence. RippleFix contains no AI and no network calls.

## Layers

```
            ┌──────────────┐  ┌────────────────┐  ┌──────────────────┐
 interfaces │ ripplefix CLI│  │ VS Code ext.   │  │ local dashboard  │
            └──────┬───────┘  └──────┬─────────┘  └────────┬─────────┘
                   │      JSON over stdio (`ripplefix … --format json`,
                   │      later a long-lived `ripplefix serve` JSON-RPC)
            ┌──────▼─────────────────────────────────────────────────┐
 core       │ ripplefix-core                                         │
            │  vcs ─► analysis ─► diff ─► (impact graph) ─► (rules)  │
            │                     ▲            ─► (patches) ─► (validate)
            └─────────────────────┼──────────────────────────────────┘
            ┌─────────────────────┴───────────┐
 plugins    │ LanguageAnalyzer (API v1)        │
            │  ripplefix-python   (Pydantic)   │
            │  ripplefix-typescript (planned)  │
            └──────────────────────────────────┘
```

The modules in parentheses are planned. See [roadmap.md](roadmap.md).

All three interfaces use the Rust core through the CLI's JSON output. The
VS Code extension and the dashboard never run their own analysis. This keeps
the behavior identical across interfaces and keeps the TypeScript code thin.

## Crates

| Crate | Responsibility |
|---|---|
| `ripplefix-core` | Domain model (`DataModel`, `Field`, `Evidence`, `Confidence`), structural diff, Git layer, analysis orchestration, plugin trait |
| `ripplefix-python` | Tree-sitter Python grammar. Classifies Pydantic models and extracts fields, defaults, and aliases |
| `ripplefix-cli` | `ripplefix` binary: argument parsing, rendering, exit codes |

## Key concepts

**Confidence.** Each change is `confirmed` (proven syntactically), `inferred`
(derived by a documented heuristic), or `unresolved` (RippleFix cannot know,
for example a dynamic alias). Automation will act on inferred facts only after
the user confirms them, and never acts on unresolved facts.

**Evidence.** Each change carries file and line references that explain it.

**Wire contract.** A Pydantic field's JSON key is its
`serialization_alias`, else its `alias`, else its name. If any of these is not
a string literal, the key is unresolved. Changes are flagged
`breaks_wire_contract` when they can alter serialized payloads or reject
payloads that were valid before.

**Cosmetic and structural changes.** Snapshots are compared ignoring source
positions. Annotations and defaults are normalized at the token level, so
comments and whitespace don't matter. A changed file whose snapshots are equal
is reported as `cosmetic`.

**Incremental analysis.** Only paths that `git diff` and `git ls-files
--others` report are parsed, and each one is parsed twice (base and working
tree).

## Rename inference rule

Within one model, a removal plus an addition is reported as an **inferred**
rename only when exactly one field was removed and exactly one was added,
both have identical normalized annotation and default, and both sit at the
same ordinal position. Any other combination is reported as separate removal
and addition, both `confirmed`.

## Security model

* Git runs as a subprocess without a shell, with `core.fsmonitor=false`,
  `--no-ext-diff`, and `--no-textconv`, so repository config cannot run
  programs during analysis. Revisions that look like options are rejected, and
  `--end-of-options` is passed.
* Files with syntax errors are reported as `unanalyzable`. RippleFix never
  diffs a partial parse.
* (Planned) The validation engine runs repository commands only when the user
  explicitly allows them in `ripplefix.toml`, and shows the exact command line.
* `unsafe` code is forbidden across the workspace.

## Architecture decisions

* [ADR 0001: Use the git CLI instead of libgit2](adr/0001-git-cli.md)
* [ADR 0002: Interfaces call the CLI and share one engine](adr/0002-single-engine.md)
