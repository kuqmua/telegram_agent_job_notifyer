# Architecture

## Overview

`server` runs Telegram long polling and HTTP health routes.

Flow:
1. `main` loads config and starts server runtime.
2. Background task `telegram::polling::run_telegram_polling` calls `getUpdates`.
3. Raw Telegram payload is converted into `InternalUpdate`.
4. Command parser maps text into `Health | Codex(String) | Unknown`.
5. Command handler sends messages through `telegram::api::TelegramApiClient`.
6. `Codex` requests execute via `codex_cli` under timeout and semaphore limits.

## Module Boundaries

- `server/src/main.rs`: bootstrap only.
- `server/src/config.rs`: environment parsing and validation.
- `server/src/telegram/api.rs`: Telegram HTTP client and API errors.
- `server/src/telegram/model.rs`: Telegram DTOs and internal update conversion.
- `server/src/telegram/commands.rs`: command parsing and names.
- `server/src/telegram/polling.rs`: polling loop, backoff, idempotency, command execution.
- `server/src/routes/health.rs`: `GET /health`, `GET /health/live`, `GET /health/ready`.
- `shared/src/lib.rs`: shared command and message helpers.

## Update Processing Diagram

```text
Telegram getUpdates
    |
    v
TelegramUpdate DTO
    |
    | convert_telegram_update_to_internal
    v
InternalUpdate(update_id, chat_id, text)
    |
    | parse_command
    +--> Health --------------> send_system_message("Health check: bot is alive")
    |
    +--> Codex(prompt) -------> semaphore acquire
    |                           -> send_system_message("Work started")
    |                           -> codex_cli exec (timeout + output limit)
    |                           -> normalize/chunk
    |                           -> send_system_message("Work finished ...")
    |
    +--> Unknown -------------> send_system_message("Unknown command")
```

## Resilience

- Exponential backoff with jitter for polling errors.
- Error split into temporary/permanent (`TelegramApiError::is_temporary`).
- Idempotency cache for processed `update_id` values.
- Request timeout at Telegram client level.
- Chunked outgoing messages with configurable max chars.
- Structured logs with `event`, `chat_id`, `update_id`, `command`, `duration_ms`, `status`, `correlation_id`.
