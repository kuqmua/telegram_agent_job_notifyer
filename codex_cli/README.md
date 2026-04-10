# codex_cli
A wrapper library over `codex`/`codex-cli`.

## What it does
- Finds the `codex` binary (`CODEX_BIN`, then `codex`/`codex-cli`).
- Checks authentication with `codex login status`.
- Runs `codex exec "<prompt>"`.

## Public API

```rust
pub fn exec_prompt(prompt: &str) -> std::io::Result<()>;
```

## Usage

```rust
codex_cli::exec_prompt("explain this repo")?;
```

In this project the call is executed from `main`.

## Requirements
- Installed Codex CLI.
- `codex login` authentication.

Install:
```bash
npm i -g @openai/codex
```

Check:
```bash
codex --version
```
