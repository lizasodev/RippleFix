# Roadmap and status

Legend: ✅ done and tested · 🚧 partially done · ⬜ not started

## First vertical slice: renaming a Pydantic response field

Scenario: in `examples/fastapi-pydantic-ts`, rename `UserBase.full_name` to
`display_name`.

| # | Acceptance criterion | Status |
|---|---|---|
| 1 | Detect the rename as a structural change with evidence and `inferred` confidence. Formatting-only edits are reported as `cosmetic` | ✅ |
| 2 | Identify affected API contracts: FastAPI routes whose `response_model`, return annotation, or typed parameter is the model or a subclass, including models from other files | ✅ |
| 3 | Identify TypeScript consumers: interfaces or types with a matching field set, plus property accesses on values of those types (Tree-sitter TypeScript) | ✅ (inferred, since there is no type checker) |
| 4 | Find tests and fixtures that use the old JSON key in literals tied to affected routes | 🚧 Python tests done. TypeScript tests are found only as key usages |
| 5 | Generate patches only for confirmed edges. Unresolved candidates are listed, not patched | ⬜ |
| 6 | Show a unified diff, and support `--dry-run` | ⬜ |
| 7 | Apply atomically. Refuse to touch files with uncommitted user changes. Support rollback through a stored reverse patch | ⬜ |
| 8 | Run configured validation commands (explicit opt-in) and report passed, failed, or not run | ⬜ |
| 9 | Final report: applied changes and unresolved cases | ⬜ |

## Phases

* **Phase 1, Foundation:** ✅ Cargo workspace, CLI, unit and end-to-end tests,
  CI (Linux, Windows, and macOS, plus an MSRV check), architecture docs.
  ⬜ The pnpm workspace is deferred until the first TypeScript package
  (the VS Code extension) exists, so it doesn't start out empty.
* **Phase 2, Core analysis:** ✅ Git change detection, Pydantic
  extraction, cross-file import resolution, project index, petgraph impact
  graph, FastAPI route extraction, TypeScript analyzer.
  ⬜ Incremental index cache.
* **Phase 3, Automation:** ⬜ Rule engine, patch system, validation, rollback,
  TOML recipes.
* **Phase 4, Developer experience:** ⬜ `ripplefix serve`, VS Code extension,
  dashboard (React, Vite, React Flow), SQLite run history.
* **Phase 5, Public beta:** ⬜ Testing on real repositories, benchmarks,
  release packaging.

## Known limitations (current)

* `analyze` (single-file change detection) does not yet see models whose
  only Pydantic base is imported from another file. `impact` does.
* Python attribute uses of a field (`payload.full_name`) are not yet listed.
* Routes registered under `APIRouter(prefix=...)` or `include_router(prefix=...)`
  are matched to tests without the prefix, so those tests are missed. No
  false links are created.
* `impact` parses all Python and TypeScript files on each run (no cache yet).
* An `alias_generator` inherited from a base class is not detected.
* `analyze` reports structural changes only for `.py` and `.pyi` files. Other changed files are counted as
  unsupported.
