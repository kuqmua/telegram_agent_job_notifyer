# Architecture

## Overview

The workspace consists of four crates:

- `server`: Telegram bot service that receives updates via long polling, exposes health and metrics HTTP endpoints, and orchestrates task execution.
- `telegram_agent_shared`: Shared domain model and shared constants for command parsing and message formatting.
- `codex_command_runtime`: Wrapper for local Codex CLI execution, including optional sandbox isolation and process control.
- `openai_command_runtime`: Wrapper for OpenAI-compatible chat completion execution.

## Request Flow

1. `server` long-polls Telegram `getUpdates`.
2. Incoming Telegram payload is converted to internal update model.
3. Authorization checks are applied (chat identifier and optional username constraints).
4. Command parser maps message text to typed `IncomingCommand`.
5. Command handler either:
- executes immediate read operation (health, list, status, queue), or
- creates and enqueues task in `TaskManager`, or
- performs OpenAI request.
6. Execution results are normalized, chunked, and returned via Telegram `sendMessage`.

## Execution Model

- A single Tokio runtime is used across the workspace.
- `TaskManager` owns task state, queueing, cancellation flags, rate limit windows, and terminal history snapshots.
- Codex task execution is bounded by semaphore (`CODEX_MAX_PARALLEL_TASKS`).
- Update processing is bounded separately (`UPDATE_MAX_PARALLEL_TASKS`).
- Worker loop tracks processed update identifiers to provide runtime idempotency.

## Reliability and Safety

- Readiness endpoint reflects polling-loop state.
- Exponential backoff with jitter is used for Telegram polling retries.
- Optional startup preflight verifies Codex binary and login status.
- Optional sandbox execution uses `bwrap` with constrained environment passthrough.

## Observability

- `/metrics` exposes Prometheus-formatted counters and gauges.
- Structured tracing events are emitted for polling, update handling, task lifecycle, and execution errors.

## Boundaries

- `server` depends on runtime crates and shared crate.
- `telegram_agent_shared` must not own runtime execution side effects.
- Runtime crates are leaf execution and integration components.
