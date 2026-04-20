# telegram-agent-job-notifyer

Telegram bot service inside a multi-crate workspace.

## What Runs Here

- `server` - Telegram bot (polling + command replies)
- `rust_codex_ci_copy/cdx_cli` - separate CLI tool copied from `rust_codex_ci`

If you want the bot, run `server` explicitly.

## Bot Commands

- `/health`
- `/help`
- `/whoami`
- `/version`

## Configuration

Minimal `.env` for bot startup:

```env
TELEGRAM_BOT_TOKEN=<your-bot-token>
```

Optional settings:

```env
TELEGRAM_API_BASE_URL=https://api.telegram.org
TELEGRAM_CHAT_ID=
TELEGRAM_ALLOWED_USERNAME=
TELEGRAM_POLL_TIMEOUT_SECONDS=30
TELEGRAM_HTTP_TIMEOUT_SECONDS=35
TELEGRAM_POLL_INITIAL_OFFSET=0
TELEGRAM_POLL_BACKOFF_MIN_MS=500
TELEGRAM_POLL_BACKOFF_MAX_MS=10000
```

Notes:

- `TELEGRAM_HTTP_TIMEOUT_SECONDS` must be greater than `TELEGRAM_POLL_TIMEOUT_SECONDS`.
- Leave `TELEGRAM_CHAT_ID` unset for first launch if you do not know it yet.

## Run Telegram Bot

```bash
cargo run -p server
```

After startup, send `/whoami` to the bot and copy `chat_id` into `.env` as `TELEGRAM_CHAT_ID` if you want chat restriction.

## Run cdx_cli (optional)

```bash
cargo run -p cdx_cli -- rust_codex_ci_copy/tasks.json
```
