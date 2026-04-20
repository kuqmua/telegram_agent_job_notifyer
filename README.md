# telegram-agent-job-notifyer

Minimal Telegram bot service.

## Features

- Long polling via Telegram Bot API (`getUpdates`)
- Basic bot commands:
  - `/health`
  - `/help`
  - `/whoami`
  - `/version`
- Optional access restrictions by chat and username

## Required environment

```env
TELEGRAM_BOT_TOKEN=<your-bot-token>
```

## Optional environment

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

## Run

```bash
cargo run
```
