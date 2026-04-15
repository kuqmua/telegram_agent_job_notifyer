# Runbook

## Local Start

1. Prepare `.env` (at minimum `TELEGRAM_BOT_TOKEN`).
2. Start service:

```bash
cargo run -p server
```

3. Verify:

```bash
curl -i http://127.0.0.1:8080/health/live
curl -i http://127.0.0.1:8080/health/ready
curl -i http://127.0.0.1:8080/metrics
```

## Incident: Bot Not Responding

1. Check readiness endpoint.
2. Inspect logs for polling and authorization events.
3. Confirm Telegram token and optional access restrictions (`TELEGRAM_CHAT_ID`, `TELEGRAM_ALLOWED_USERNAME`).
4. Verify outbound network access to Telegram API.

## Incident: Tasks Stuck In Queue

1. Check `task_queue_depth` and task lifecycle metrics.
2. Validate `CODEX_MAX_PARALLEL_TASKS` and `UPDATE_MAX_PARALLEL_TASKS` values.
3. Check Codex runtime availability and timeout settings (`CODEX_TIMEOUT_SECONDS`).

## Incident: Codex Execution Fails On Startup

1. Verify configured binary path (`CODEX_BINARY_PATH` or `CODEX_BIN`).
2. Run login check manually:

```bash
codex login status
```

3. If startup preflight must be bypassed temporarily, set:

```env
CODEX_REQUIRE_LOGIN_STATUS=false
```

## Incident: Sandbox Failures

1. Ensure `CODEX_SANDBOX_LAUNCHER_PATH` points to `bwrap` absolute path.
2. Ensure `CODEX_SANDBOX_WORKSPACE_ROOT` exists and is absolute.
3. Validate `CODEX_SANDBOX_LAUNCHER_ARGS` policy against `CODEX_SANDBOX_ALLOW_CUSTOM_LAUNCHER_ARGS`.

## Recovery Checklist

1. Confirm polling resumes and `/health/ready` is `200`.
2. Confirm `task_queue_depth` returns to expected level.
3. Execute `/health` and one `/codex` task from authorized user.
4. Capture timeline and root cause in incident notes.
