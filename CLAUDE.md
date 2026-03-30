# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Check Commands

```bash
cargo fmt                                              # format
cargo clippy --all-targets --all-features              # lint (all lints are deny in workspace config)
cargo build                                            # build all
cargo test                                             # test all
cargo test -p <crate_name>                             # test single crate
cargo test -p <crate_name> <test_name>                 # test single test
```

Always run `cargo fmt` and `cargo clippy --all-targets --all-features` before finishing work.

## Toolchain

Rust nightly, edition 2024, MSRV 1.85.

## Architecture

Rust workspace with `server` as the sole member. Workspace root `Cargo.toml` owns:
- All crate.io dependency versions (pinned with `=`, default-features disabled)
- All lint configuration (~280 rust lints + ~800 clippy lints, nearly everything is `deny`)
- Shared package metadata (version, edition, license, rust-version)

Member crates inherit via `version.workspace = true`, `dependency.workspace = true`, `[lints] workspace = true`.

## Key Conventions (from AGENTS.md)

- No `unwrap()`. Use `expect()` only in binaries/tests/proc-macros, with message containing **8 first symbols of a random UUID v4**.
- No `unsafe`. No Axum middleware layers — call reusable functions explicitly in route handlers.
- Errors: enums + `thiserror`. Never swallow `Result`.
- Dependencies: workspace-level only, disable default features, prefer `std` over external crates.
- No blank lines between code. No commented-out dead code. Minimal diffs.
- Use abbreviations in names. Keep generated functions/closures inside usage scope.
- Public API: keep minimal, don't change without instruction.

## Workspace Dependencies Stack

axum, tokio, sqlx (postgres), serde/serde_json, tracing, metrics/prometheus, jsonwebtoken, argon2, reqwest, utoipa (swagger), chrono, uuid.
