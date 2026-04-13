# Server

`server` is a long-polling Telegram bot service.

## Endpoints

- `GET /health/live` - process is alive.
- `GET /health/ready` - polling loop readiness.
- `GET /health` - alias of readiness.
- `GET /metrics` - Prometheus metrics.

## Runtime Behavior

- Polling via `getUpdates` with exponential backoff + jitter.
- Runtime idempotency guard for duplicate `update_id` values.
- Explicit command model: `Health`, `Codex(String)`, `Unknown`.
- Additional commands: `/status`, `/list`, `/active`, `/retry`, `/cancel`, `/whoami`, `/version`.
- `codex` execution is limited by semaphore and timeout.
- Outgoing messages are normalized and chunked by max length.

## Required Environment Variables

- `TELEGRAM_BOT_TOKEN`

## Optional Environment Variables

- `TELEGRAM_CHAT_ID`
- `TELEGRAM_ALLOWED_USERNAME` (optional, username without `@` or with it; matched case-insensitively)
- `TELEGRAM_ADMIN_USERNAMES` (optional, comma-separated usernames with or without `@`)
- `HOST` (default `0.0.0.0`)
- `PORT` (default `8080`)
- `TELEGRAM_POLL_TIMEOUT_SECONDS` (default `30`)
- `TELEGRAM_POLL_BACKOFF_MIN_MS` (default `500`)
- `TELEGRAM_POLL_BACKOFF_MAX_MS` (default `10000`)
- `TELEGRAM_POLL_INITIAL_OFFSET` (default `0`)
- `TELEGRAM_HTTP_TIMEOUT_SECONDS` (default `15`)
- `TELEGRAM_API_BASE_URL` (default `https://api.telegram.org`)
- `CODEX_MAX_PARALLEL_TASKS` (default `2`)
- `CODEX_BINARY_PATH` (optional, absolute path to `codex` binary)
- `CODEX_REQUIRE_LOGIN_STATUS` (default `true`; startup fails if `codex login status` fails)
- `CODEX_SANDBOX_ENABLED` (default `false`, enables isolated execution mode)
- `CODEX_SANDBOX_ALLOW_NETWORK` (default `false`; when `false`, `bwrap` starts with network namespace isolation)
- `CODEX_SANDBOX_ALLOW_CUSTOM_LAUNCHER_ARGS` (default `false`; when `false`, non-empty `CODEX_SANDBOX_LAUNCHER_ARGS` is rejected)
- `CODEX_SANDBOX_WORKSPACE_ROOT` (required when `CODEX_SANDBOX_ENABLED=true`, absolute path)
- `CODEX_SANDBOX_AUTO_CLEANUP` (default `true`; when `false`, sandbox `job_*` directories are kept after task completion)
- `CODEX_SANDBOX_LAUNCHER_PATH` (required when `CODEX_SANDBOX_ENABLED=true`, must point to `bwrap`, absolute path)
- `CODEX_SANDBOX_LAUNCHER_ARGS` (optional, comma-separated launcher arguments)
- `CODEX_SANDBOX_ALLOWED_ENV` (optional, comma-separated env allow-list for sandbox mode)
- `CODEX_TIMEOUT_SECONDS` (default `120`)
- `CODEX_OUTPUT_MAX_BYTES` (default `65536`)
- `UPDATE_MAX_PARALLEL_TASKS` (default `64`)
- `TELEGRAM_MESSAGE_MAX_CHARACTERS` (default `3500`)
- `PROCESSED_UPDATE_CACHE_SIZE` (default `4096`)
- `TASK_RATE_LIMIT_PER_MINUTE` (default `30`)
- `TASK_LIST_MAX_ITEMS` (default `10`)
- `TASK_HISTORY_MAX_SIZE` (default `2048`)
- `TASK_HISTORY_FILE_PATH` (optional, JSONL append-only task terminal history)
- `PROMPT_MAX_CHARACTERS` (default `8000`, hard limit before task enters queue)
- `TASK_QUEUE_MAX_WAIT_SECONDS` (default `120`, maximum queue wait before cancellation)

## Monitoring

- `task_queue_depth` metric is exposed on `/metrics`.
- Example alert rule is in `server/monitoring/alerts.yml`.

## Container Notes

- Docker assets are in `server/`: `server/Dockerfile`, `server/docker-compose.yml`.
- Compose service mounts host `codex` binary to `/usr/local/bin/codex`.
- Compose setup profile `codex-login` persists Codex auth in Docker volume `codex-home`.
