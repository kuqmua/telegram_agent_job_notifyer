# Telegram Agent Job Notifyer

Workspace crates:
- `server` - Telegram bot service using long polling (`/health`, `/health/live`, `/health/ready`)


## Quick Start

1. Prepare `.env`:

```env
TELEGRAM_BOT_TOKEN=123456789:replace_with_real_token
TELEGRAM_CHAT_ID=123456789
TELEGRAM_ALLOWED_USERNAME=kuqmua
HOST=127.0.0.1
PORT=8080
TELEGRAM_POLL_TIMEOUT_SECONDS=30
TELEGRAM_POLL_BACKOFF_MIN_MS=500
TELEGRAM_POLL_BACKOFF_MAX_MS=10000
TELEGRAM_HTTP_TIMEOUT_SECONDS=15
TELEGRAM_API_BASE_URL=https://api.telegram.org
CODEX_MAX_PARALLEL_TASKS=2
CODEX_BINARY_PATH=/usr/local/bin/codex
CODEX_SANDBOX_ENABLED=false
CODEX_SANDBOX_WORKSPACE_ROOT=/tmp/telegram_agent_codex_sandbox
CODEX_SANDBOX_LAUNCHER_PATH=
CODEX_SANDBOX_LAUNCHER_ARGS=
CODEX_SANDBOX_ALLOWED_ENV=PATH,HOME,CODEX_HOME,OPENAI_API_KEY,HTTPS_PROXY,HTTP_PROXY,NO_PROXY
CODEX_TIMEOUT_SECONDS=120
CODEX_OUTPUT_MAX_BYTES=65536
UPDATE_MAX_PARALLEL_TASKS=64
TELEGRAM_MESSAGE_MAX_CHARACTERS=3500
PROCESSED_UPDATE_CACHE_SIZE=4096
TELEGRAM_ADMIN_USERNAMES=owner_user,second_admin
TASK_RATE_LIMIT_PER_MINUTE=30
TASK_LIST_MAX_ITEMS=10
TASK_HISTORY_MAX_SIZE=2048
TASK_HISTORY_FILE_PATH=/tmp/telegram_agent_task_history.jsonl
```

2. Run server:

```bash
cargo run -p server
```

3. Check health:

```bash
curl -i http://127.0.0.1:8080/health/live
curl -i http://127.0.0.1:8080/health/ready
```

## Telegram Commands

- `/health`
- `/help`
- `/codex <prompt>`
- `/status <task_id>`
- `/list`
- `/active`
- `/cancel <task_id>`
- `/retry <task_id>`
- `/limits`

## Operations

- Architecture: `docs/architecture.md`
- Incident handling: `docs/runbook.md`

## Security Notes

- Never commit real `TELEGRAM_BOT_TOKEN` values.
- Rotate `TELEGRAM_BOT_TOKEN` if it was exposed.
- Keep `.env` local only and do not paste token values into logs/issues.
- Server config validation fails fast on empty or placeholder-like token values.
- To allow only one Telegram account, set both `TELEGRAM_CHAT_ID` and `TELEGRAM_ALLOWED_USERNAME`.
