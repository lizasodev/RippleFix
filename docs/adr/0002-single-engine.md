# ADR 0002: Interfaces call the CLI and share one engine

Status: accepted (2026-11-09)

## Context

RippleFix ships a CLI, a VS Code extension, and a local dashboard. If the
TypeScript interfaces reimplemented analysis, or used WASM builds with
different capabilities, their results could drift apart.

## Decision

The Rust binary is the only engine. The interfaces call it and consume its
versioned JSON output. Later, a `ripplefix serve` subcommand will provide
JSON-RPC over stdio for long-lived sessions. The dashboard will be served
locally by the same binary and bound to `127.0.0.1` only.

## Consequences

* The JSON schema is a public contract and needs versioning and snapshot tests.
* The extension must find or bundle a platform-specific binary, which the
  release workflow must produce for each target.
* We avoid a Node-side parser stack and keep the dependency surface small.
