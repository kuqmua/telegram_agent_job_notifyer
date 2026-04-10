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
- `codex` execution is limited by semaphore and timeout.
- Outgoing messages are normalized and chunked by max length.

## Required Environment Variables

- `TELEGRAM_BOT_TOKEN`

## Optional Environment Variables

- `TELEGRAM_CHAT_ID`
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
- `CODEX_TIMEOUT_SECONDS` (default `120`)
- `CODEX_OUTPUT_MAX_BYTES` (default `65536`)
- `UPDATE_MAX_PARALLEL_TASKS` (default `64`)
- `TELEGRAM_MESSAGE_MAX_CHARACTERS` (default `3500`)
- `PROCESSED_UPDATE_CACHE_SIZE` (default `4096`)
