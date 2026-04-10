# Codex CLI flags (`codex-cli 0.118.0`)

This file lists flags available in the locally installed version.

## Important

- There is no separate `codex cli` subcommand.
- `codex cli --help` prints the same help as `codex --help`.

## Global flags for `codex`

- `-c, --config <key=value>`: override values from `~/.codex/config.toml`.
- `--enable <FEATURE>`: enable a feature flag (repeatable).
- `--disable <FEATURE>`: disable a feature flag (repeatable).
- `--remote <ADDR>`: connect TUI to remote app server (`ws://` or `wss://`).
- `--remote-auth-token-env <ENV_VAR>`: env var name with bearer token for remote app server.
- `-i, --image <FILE>...`: attach images to the initial prompt.
- `-m, --model <MODEL>`: select model.
- `--oss`: use local open-source model provider.
- `--local-provider <OSS_PROVIDER>`: pick local provider (`lmstudio` or `ollama`).
- `-p, --profile <CONFIG_PROFILE>`: config profile from `config.toml`.
- `-s, --sandbox <SANDBOX_MODE>`: sandbox mode (`read-only`, `workspace-write`, `danger-full-access`).
- `-a, --ask-for-approval <APPROVAL_POLICY>`: command approval policy (`untrusted`, `on-failure`, `on-request`, `never`).
- `--full-auto`: shorthand for `-a on-request` + `--sandbox workspace-write`.
- `--dangerously-bypass-approvals-and-sandbox`: run without approvals and without sandbox (very dangerous).
- `-C, --cd <DIR>`: set agent working directory.
- `--search`: enable live web search tool.
- `--add-dir <DIR>`: extra writable directories.
- `--no-alt-screen`: disable terminal alternate screen mode.
- `-h, --help`: show help.
- `-V, --version`: show version.

## Often useful flags

- Safe daily mode: `-s workspace-write -a on-request`
- Script and CI mode: `codex exec --json --output-last-message <FILE> --color never`
- Ephemeral run without session persistence: `codex exec --ephemeral`
- Run outside git repository: `codex exec --skip-git-repo-check`
- Multiple writable directories for monorepo: `--add-dir <DIR>`

## `codex exec`

- `--skip-git-repo-check`: allow run outside git repository.
- `--ephemeral`: do not persist session files.
- `--output-schema <FILE>`: JSON Schema for final model response shape.
- `--color <always|never|auto>`: color mode.
- `--json`: emit JSONL events to stdout.
- `-o, --output-last-message <FILE>`: write final agent message to file.

## `codex review`

- `--uncommitted`: review staged, unstaged, and untracked changes.
- `--base <BRANCH>`: review diff against base branch.
- `--commit <SHA>`: review changes from a specific commit.
- `--title <TITLE>`: optional title in review summary.

## `codex resume`

- `--last`: continue most recent session without picker.
- `--all`: show all sessions, without current cwd filtering.
- `--include-non-interactive`: include non-interactive sessions in picker and `--last`.

## `codex mcp`

### `codex mcp add`

- `codex mcp add <NAME> --url <URL>`: add streamable HTTP MCP server.
- `codex mcp add <NAME> -- <COMMAND>...`: add stdio MCP server command.
- `--env <KEY=VALUE>`: env vars for stdio MCP server.
- `--bearer-token-env-var <ENV_VAR>`: bearer token env var for HTTP MCP server.

### `codex mcp list`

- `--json`: print configured servers as JSON.

### `codex mcp login`

- `--scopes <SCOPE,SCOPE>`: OAuth scopes for authentication.

## `codex apply`

- `codex apply <TASK_ID>`: apply latest task diff to current git working tree.
