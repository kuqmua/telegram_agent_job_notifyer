# Runbook

## Health Endpoints

- `GET /health/live`: process liveness.
- `GET /health/ready`: readiness of polling subsystem.
- `GET /health`: alias of readiness.

## Incident: `409 Conflict` from Telegram

Symptoms:
- Polling errors with Telegram HTTP 409.

Cause:
- Webhook was configured for the same bot.

Fix:
```bash
source .env
curl "https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/deleteWebhook?drop_pending_updates=true"
```

Validate:
```bash
curl "https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/getWebhookInfo"
```
`url` must be empty.

## Incident: Telegram rate limit (`429`)

Symptoms:
- `telegram http status 429` in logs.

Expected behavior:
- Polling loop backs off automatically with jitter.

Actions:
1. Verify no message flood from clients.
2. Reduce command traffic or increase throttling upstream.
3. Monitor `polling_error` logs until recovery.

## Incident: empty or malformed updates

Symptoms:
- `event=update_ignored status=invalid_payload` in logs.

Expected behavior:
- Service keeps running; invalid updates are skipped.

Actions:
1. Inspect raw Telegram update payload with `getUpdates` manually.
2. Confirm bot receives text messages from the intended chat.
3. If payload shape changed, update DTO conversion and tests.

## Incident: codex task timeout

Symptoms:
- Result message includes `codex timed out`.

Actions:
1. Increase `CODEX_TIMEOUT_SECONDS` if needed.
2. Check codex authentication and host health.
3. Review `codex_execution_start`/`codex_execution_finish` durations.
