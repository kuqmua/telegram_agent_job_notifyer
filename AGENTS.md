## WHAT AGENT MUST DO

- Place shared logic in a dedicated shared crate.
- Use workspace-level dependencies.
- Add dependencies only when prompt explicitly requests it.
- Disable default features unless required.
- Prefer `std` over external crates.
- Declare crates.io dependencies only in workspace.dependencies.
- Use dependency.workspace = true for dependencies in workspace projects
- Prefer borrowing over cloning, especially for large structures.
- Use `Arc` only for cross-thread sharing.
- Use `Mutex` only for interior mutability.
- Prefer immutable data.
- Avoid memory leaks via static state.
- Use enums and `thiserror` for errors.
- Use a single async runtime across workspace.
- Keep trait bounds explicit.
- Use trait objects only when dynamic dispatch is required.
- Keep public API minimal.
- Add unit tests for public logic.
- Use test helpers for repeated setup.
- Keep tests deterministic.
- If error message contains 8 random symbols then search workspace for that id.
- Avoid allocations inside hot loops.
- Preserve behavior unless change is requested.
- In SQL queries, always reuse table and column name constants (`table_names::*`, `COLUMN_*`, `FIELD_*`, `TABLE_*`) instead of hardcoded string literals for schema identifiers. For every new or edited SQL query (including idempotency, auth, handlers, models, and tests), do not inline table/column identifiers in query text; add or reuse a shared constant first and then reference it in `format!`.
- Reuse shared error message constants instead of duplicating hardcoded error strings across handlers, models, and tests.
- Reuse shared error message parts (prefixes/suffixes/field fragments) via common constants or builders; do not duplicate near-identical hardcoded error text variants.
- Never use Axum middleware layers (`.layer(from_fn(...))`) for cross-cutting concerns like auth, rate limiting, idempotency, or validation. Instead, call a reusable function explicitly in each route handler. This keeps error types visible in the handler signature and avoids hidden control flow.
- Keep generated functions and closures inside usage scope.
- Do not create a separate function for logic used only once in regular code; keep it inline. Exceptions: route handlers and closely related routing code, middleware code, entrypoint code (`main` and startup wiring), and tests may use single-use helper functions when this clearly improves readability, structure, or reduces duplication.
- `expect()` messages must contain **8 first symbols from random UUID v4**.
- Do not use abbreviations in names for variables, functions, methods, traits, constants, structs, enums, modules, type aliases, fields, or parameters; use explicit, full names. The same rule applies to database schema names (tables, columns, enums, constraints): do not introduce abbreviated names there either. For constants, use explicit full-word prefixes/tokens: `COLUMN_` instead of `COL_`, `PERMISSION_` instead of `PERM_`, `MESSAGE_` instead of `MSG_`, `PASSWORD` instead of `PWD`, `GEOMETRY` instead of `GEOM`, `VIRTUAL_USER` instead of `VU`.
- Use concrete crate types from crates.io in workspace Cargo.toml.
- Use `*CRATE NAME*.workspace = true` for workspace crate dependencies in Cargo.toml.
- Keep validation thresholds local (`let`/local const) when they are used in a single scope, and reuse those local values in error messages.
- Formatting is defined by `cargo fmt`; do not enforce manual formatting rules that conflict with formatter output.
- Prefer imports over absolute paths in type signatures and expressions; avoid `#[allow(clippy::absolute_paths)]` by refactoring imports.
- For mass refactors (regex/sed/perl/global rename), first limit scope to an explicit file list, then review full `git diff` before completion.
- Do not add lint `allow` attributes (`#[allow(...)]` or `#![allow(...)]`) to bypass workspace lints, including in tests.
- Do not use `as` numeric conversions; use `From`/`TryFrom` and explicit bounds checks.
- Do not change external contracts without explicit request: environment variable names, HTTP header names, JSON field names, and route paths.
- When renaming constants, keep external contract string values unchanged (rename Rust identifiers only, not protocol/schema strings).
- Before completion, run checks in this exact order: `cargo fmt` -> `cargo clippy --all-targets --all-features -- -D warnings` -> `cargo test`. If full `cargo test` is not feasible, run affected test targets and explicitly report what was skipped and why.
- In the final report, always list executed verification commands and their outcomes; if any required check was skipped, state it explicitly with reason.
- Do not use indexing access like `[0]` or `[1]` even in tests; use `first()`/`get()` with explicit handling.
- Do not mask failures with `unwrap_or_default()`/`unwrap_or(...)` where this can hide errors; prefer `Result` propagation and explicit `expect()` with 8-char id.
- Delete unused code immediately instead of keeping it behind `#[allow(unused_...)]`.

## WHAT AGENT MUST NOT DO

- Merge unrelated crates.
- Break architecture boundaries or introduce hidden coupling.
- Edit Cargo.toml of unrelated crates.
- Add new crates unless explicitly requested.
- Silence clippy without justification.
- Use `#[allow(dead_code)]`.
- Leave commented dead code.
- Commit debug prints.
- Use `unwrap()`.
- Do not use `expect()` or `panic!()` in library/runtime code except in `proc-macro` or generated test code inside `quote!`.
- Ignore `Result` or swallow errors.
- Use or write `unsafe`.
- Assume `Send` or `Sync` without proof.
- Use outdated versions in case of adding new crate.
- Block async executors.
- Hold locks across `.await`.
- Mix async runtimes.
- Ignore cancellation safety.
- Depend on external services in tests.
- Use flaky time-based tests.
- Change public API without instruction.
- Leak generics to users.
- Rename public items casually.
- Change semantics silently.

## Run before completion

```bash
cargo fmt
```

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

```bash
cargo test
```

## Toolchain note

- This repository is intended for the latest Rust nightly toolchain.
