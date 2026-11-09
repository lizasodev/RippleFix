# Standards alignment

This document maps RippleFix engineering practices to recognized standards
and frameworks.

> **Important.** RippleFix is **not certified** against any standard.
> ISO/IEC 27001 certifies an organization's information security management
> system (ISMS) through an accredited audit, so a software repository cannot
> be "ISO 27001 compliant" by itself. What this repository can do, and does,
> is implement the secure-development controls those standards expect, so
> that an organization adopting RippleFix can show auditors evidence of them.
> Each claim below links to the file that implements it, so it can be checked.

## ISO/IEC 27001:2022, Annex A technological controls

| Control | How RippleFix addresses it | Evidence |
|---|---|---|
| A.8.4 Access to source code | `CODEOWNERS` requires maintainer review of CI, the policy, and the Git execution layer. Branch protection is recommended (see below) | `.github/CODEOWNERS` |
| A.8.8 Management of technical vulnerabilities | RustSec advisories checked on every pull request. Dependabot updates. Private vulnerability reporting | `deny.toml`, `.github/dependabot.yml`, `SECURITY.md` |
| A.8.9 Configuration management | Lockfile committed and enforced (`--locked`). Toolchain pinned. Actions pinned to commit SHAs | `Cargo.lock`, `rust-toolchain.toml`, `.github/workflows/*.yml` |
| A.8.12 Data leakage prevention | No network access. No telemetry. Analysis is entirely local | [architecture.md](architecture.md#security-model) |
| A.8.25 Secure development life cycle | Documented workflow, mandatory review, tests, and static analysis on every change | `CONTRIBUTING.md`, `.github/pull_request_template.md` |
| A.8.26 Application security requirements | Threat model and security commitments (no unauthorized command execution, never overwrite user work) | `SECURITY.md`, [architecture.md](architecture.md#security-model) |
| A.8.27 Secure system architecture | Single deterministic engine. `unsafe` forbidden. Least-privilege subprocesses | ADRs in `docs/adr/` |
| A.8.28 Secure coding | `#![forbid(unsafe_code)]` workspace lint, clippy with `-D warnings`, CodeQL. Untrusted input (revisions, repo config) validated | `Cargo.toml` `[workspace.lints]`, `.github/workflows/codeql.yml`, `crates/ripplefix-core/src/vcs.rs` |
| A.8.29 Security testing | Tests for option injection and malformed input. End-to-end tests on a real Git repository | `crates/ripplefix-core/src/vcs.rs`, `crates/ripplefix-cli/tests/` |
| A.8.31 Separation of environments | CI runs in ephemeral GitHub-hosted runners with read-only tokens and `persist-credentials: false` | `.github/workflows/ci.yml` |
| A.8.32 Change management | Conventional commits, SemVer, changelog, pull request template | `CONTRIBUTING.md`, `CHANGELOG.md` |

## NIST SP 800-218 Secure Software Development Framework (SSDF)

| Practice | Implementation |
|---|---|
| PO.1 Define security requirements | `SECURITY.md` scope, architecture security model |
| PO.3 Implement supporting toolchains | CI: fmt, clippy, tests on 3 operating systems, MSRV, cargo-deny, CodeQL |
| PS.1 Protect all forms of code | CODEOWNERS, SHA-pinned actions, read-only workflow tokens |
| PS.2 Provide a mechanism to verify release integrity | **Planned:** signed release artifacts with SLSA build provenance and a CycloneDX SBOM (Phase 5) |
| PW.4 Reuse well-secured software | Minimal dependencies. Only crates.io sources. License allowlist |
| PW.7 Review and analyze code | Required review, clippy, CodeQL |
| PW.8 Test executable code | Unit and end-to-end tests |
| RV.1 Identify and confirm vulnerabilities | RustSec on every pull request, weekly CodeQL, private reporting |

## ISO/IEC 25010 (product quality), selected characteristics

* **Functional correctness:** no relationship is presented as confirmed
  without syntactic proof (`confirmed` / `inferred` / `unresolved` on
  every result).
* **Reliability (fault tolerance):** files with syntax errors are reported
  as unanalyzable, never partially diffed.
* **Portability:** CI on Linux, Windows, and macOS. Git paths are
  normalized to `/`.
* **Maintainability (modularity):** separate crates per concern, with a
  versioned plugin API.

## Data protection (GDPR / LOPDGDD)

RippleFix processes source code locally and does not collect, store, or
transmit personal data. No telemetry exists. If run history is added
(SQLite, Phase 4), it will stay in the repository's `.ripplefix/` directory,
under the user's control.

## Licensing

Apache-2.0 for RippleFix. Dependency licenses are restricted to a permissive
allowlist enforced by `cargo deny check licenses`.

## Recommended repository settings (to be applied on GitHub)

These are platform settings and cannot be stored in the repository:

1. Branch protection on `main`: require a pull request, 1 approving review,
   CODEOWNERS review, the CI and CodeQL checks, and linear history. Block
   force pushes.
2. Enable private vulnerability reporting, Dependabot alerts, secret
   scanning, and push protection.
3. Require 2FA for all collaborators.
4. Set the Actions default `GITHUB_TOKEN` permissions to read-only.
