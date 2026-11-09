# Security policy

## Reporting a vulnerability

Please do not open public issues for security problems. Use GitHub's
**private vulnerability reporting** for this repository ("Security" tab →
"Report a vulnerability"). We aim to acknowledge reports within 3 business
days.

## Scope and design commitments

RippleFix reads untrusted repositories, so the following count as security
bugs:

* Running any repository-defined program (Git hooks, fsmonitor, textconv,
  build scripts, test commands) without explicit user authorization.
* Writing outside the repository, or modifying files with uncommitted user
  changes.
* Any network access. RippleFix is local-only.

See [the security model in docs/architecture.md](docs/architecture.md#security-model).

## Supported versions

Before 1.0, only the latest release receives fixes.
