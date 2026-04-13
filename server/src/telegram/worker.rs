use std::{
    collections::{HashSet, VecDeque},
    hint::black_box,
    sync::{
        Arc,
        atomic::Ordering,
        mpsc::{Receiver, channel},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::{
    sync::{TryAcquireError, watch},
    task::{JoinSet, spawn_blocking},
    time::{Instant, sleep, timeout},
};

use crate::{
    runtime::ServiceState,
    settings::ServiceConfiguration,
    shared::{
        CodexExecutionIsolation, CodexTaskStatus, IncomingCommand, PromptExecutionOutcome,
        SYSTEM_MESSAGE_CODEX_BUSY, SYSTEM_MESSAGE_CODEX_CANCELLED, SYSTEM_MESSAGE_CODEX_FINISHED,
        SYSTEM_MESSAGE_CODEX_PROCESS_USAGE, SYSTEM_MESSAGE_CODEX_QUEUED,
        SYSTEM_MESSAGE_CODEX_STARTED, SYSTEM_MESSAGE_CODEX_TIMED_OUT, SYSTEM_MESSAGE_CODEX_USAGE,
        SYSTEM_MESSAGE_HEALTHY, SYSTEM_MESSAGE_HELP, SYSTEM_MESSAGE_INVALID_COMMAND_ARGUMENTS,
        SYSTEM_MESSAGE_TASK_ACCESS_DENIED, SYSTEM_MESSAGE_TASK_NOT_FOUND,
        SYSTEM_MESSAGE_TASK_PROMPT_TOO_LONG, SYSTEM_MESSAGE_TASK_QUEUE_WAIT_EXCEEDED,
        SYSTEM_MESSAGE_TASK_RATE_LIMITED, SYSTEM_MESSAGE_UNKNOWN_COMMAND,
        SYSTEM_MESSAGE_USERNAME_REQUIRED, TaskCreationRequest, TaskOwner, TaskSummary,
        exec_prompt_capture_limited_with_binary_and_control_with_json_output_and_progress,
        format_system_message, normalize_codex_output, split_text_into_chunks,
    },
    task_manager::{TaskCancellationResult, TaskCreationError, TaskLookupError, TaskRetryLookup},
    telegram::{
        api::TelegramApiError,
        commands::{command_name, parse_command},
        model::{InternalUpdate, convert_telegram_update_to_internal},
    },
};

const TASK_PROMPT_PROCESS_OUTPUT_MARKER: &str = "__task_prompt_process_output__: ";
const SYSTEM_MESSAGE_CODEX_PROCESS_STREAM: &str = "Codex process output";

struct PollingBackoff {
    current_delay_milliseconds: u64,
    maximum_delay_milliseconds: u64,
    minimum_delay_milliseconds: u64,
}

impl PollingBackoff {
    const fn reset(&mut self) {
        self.current_delay_milliseconds = self.minimum_delay_milliseconds;
    }

    fn take_delay(&mut self) -> Duration {
        let jitter_window = self.current_delay_milliseconds >> 2u32;
        let jitter_value = if jitter_window == 0 {
            0
        } else {
            let nanoseconds_since_epoch = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0u128, |duration| duration.as_nanos());
            let timestamp_value = u64::try_from(nanoseconds_since_epoch).unwrap_or(u64::MAX);
            let bitmask = jitter_window.next_power_of_two().saturating_sub(1);
            let candidate_value = timestamp_value & bitmask;
            candidate_value.min(jitter_window)
        };
        let delay_with_jitter = self.current_delay_milliseconds.saturating_add(jitter_value);
        self.current_delay_milliseconds = self
            .current_delay_milliseconds
            .saturating_mul(2)
            .clamp(self.minimum_delay_milliseconds, self.maximum_delay_milliseconds);
        Duration::from_millis(delay_with_jitter)
    }
}

struct ProcessedUpdateCache {
    insertion_order: VecDeque<i64>,
    known_identifiers: HashSet<i64>,
    maximum_size: usize,
}

impl ProcessedUpdateCache {
    fn contains(&self, update_identifier: i64) -> bool {
        self.known_identifiers.contains(&update_identifier)
    }

    fn insert(&mut self, update_identifier: i64) {
        if self.known_identifiers.insert(update_identifier) {
            self.insertion_order.push_back(update_identifier);
        }
        while self.insertion_order.len() > self.maximum_size {
            if let Some(oldest_update_identifier) = self.insertion_order.pop_front() {
                let _was_present = self.known_identifiers.remove(&oldest_update_identifier);
            }
        }
    }
}

pub async fn run_updates_loop(
    runtime_state: ServiceState,
    runtime_settings: Arc<ServiceConfiguration>,
    shutdown_receiver: watch::Receiver<bool>,
) {
    tracing::info!(event = "polling_start", status = "ok");
    refresh_task_queue_depth_metric(&runtime_state).await;
    let queued_task_dispatch_data = runtime_state
        .task_manager()
        .queued_task_dispatch_data()
        .await;
    if !queued_task_dispatch_data.is_empty() {
        tracing::info!(
            event = "queued_tasks_restored",
            queued_count = queued_task_dispatch_data.len(),
            status = "ok"
        );
        for (task_identifier, chat_identifier) in queued_task_dispatch_data {
            spawn_task_execution(
                &runtime_state,
                &runtime_settings,
                chat_identifier,
                0,
                runtime_state.next_correlation_identifier(),
                task_identifier,
            );
        }
    }
    let mut update_offset = runtime_settings.polling_initial_offset;
    let mut processed_update_cache = ProcessedUpdateCache {
        insertion_order: VecDeque::new(),
        known_identifiers: HashSet::new(),
        maximum_size: runtime_settings.processed_update_cache_size,
    };
    let mut polling_backoff = PollingBackoff {
        current_delay_milliseconds: runtime_settings.polling_backoff_min_milliseconds,
        maximum_delay_milliseconds: runtime_settings.polling_backoff_max_milliseconds,
        minimum_delay_milliseconds: runtime_settings.polling_backoff_min_milliseconds,
    };
    let mut update_tasks = JoinSet::new();
    'polling_loop: while !*shutdown_receiver.borrow() {
        refresh_task_queue_depth_metric(&runtime_state).await;
        while let Some(join_result) = update_tasks.try_join_next() {
            if let Err(join_error) = join_result {
                tracing::error!(
                    event = "update_task_join_error",
                    status = "error",
                    error = join_error.to_string()
                );
            }
        }
        let poll_started_at = Instant::now();
        runtime_state.metrics().increment_polling_request_total();
        let polling_result = runtime_state
            .telegram_client()
            .get_updates(update_offset, runtime_settings.polling_timeout_seconds)
            .await;
        match polling_result {
            Ok(telegram_updates) => {
                let polling_duration_milliseconds = poll_started_at.elapsed().as_millis();
                runtime_state
                    .metrics()
                    .record_polling_duration_milliseconds(polling_duration_milliseconds);
                runtime_state.metrics().increment_polling_success_total();
                tracing::info!(
                    event = "updates_received",
                    update_count = telegram_updates.len(),
                    duration_ms = polling_duration_milliseconds,
                    status = "ok"
                );
                runtime_state.set_polling_ready(true);
                polling_backoff.reset();
                for telegram_update in telegram_updates {
                    if telegram_update.update_id >= update_offset {
                        update_offset = telegram_update.update_id.saturating_add(1);
                    }
                    if processed_update_cache.contains(telegram_update.update_id) {
                        runtime_state.metrics().increment_update_duplicate_total();
                        tracing::info!(
                            event = "update_duplicate",
                            update_id = telegram_update.update_id,
                            status = "skipped"
                        );
                        continue;
                    }
                    processed_update_cache.insert(telegram_update.update_id);
                    let Some(internal_update) =
                        convert_telegram_update_to_internal(telegram_update)
                    else {
                        tracing::info!(event = "update_ignored", status = "invalid_payload");
                        continue;
                    };
                    if runtime_state.is_chat_authorized(internal_update.chat_identifier)
                        && internal_update.sender_username.is_none()
                        && runtime_state.requires_sender_username_for_access()
                    {
                        let correlation_identifier = runtime_state.next_correlation_identifier();
                        send_message_or_log(
                            &runtime_state,
                            &runtime_settings,
                            internal_update.chat_identifier,
                            internal_update.update_identifier,
                            "authorization",
                            &correlation_identifier,
                            SYSTEM_MESSAGE_USERNAME_REQUIRED,
                        )
                        .await;
                        tracing::warn!(
                            event = "update_username_required",
                            chat_id = internal_update.chat_identifier,
                            update_id = internal_update.update_identifier,
                            status = "ignored"
                        );
                        continue;
                    }
                    if !runtime_state.is_update_authorized(
                        internal_update.chat_identifier,
                        internal_update.sender_username.as_deref(),
                    ) {
                        tracing::warn!(
                            event = "update_not_authorized",
                            chat_id = internal_update.chat_identifier,
                            sender_username = internal_update
                                .sender_username
                                .as_deref()
                                .map_or("<missing>", |sender_username| sender_username),
                            update_id = internal_update.update_identifier,
                            status = "ignored"
                        );
                        continue;
                    }
                    let update_processing_permit = loop {
                        if *shutdown_receiver.borrow() {
                            break 'polling_loop;
                        }
                        match runtime_state.try_acquire_update_processing_permit() {
                            Ok(permit) => break permit,
                            Err(try_acquire_error) => match try_acquire_error {
                                TryAcquireError::Closed => {
                                    tracing::error!(
                                        event = "update_semaphore_error",
                                        status = "error",
                                        error = String::from("semaphore closed")
                                    );
                                    continue 'polling_loop;
                                }
                                TryAcquireError::NoPermits => {
                                    sleep(Duration::from_millis(5)).await;
                                }
                            },
                        }
                    };
                    let command_runtime_state = runtime_state.clone();
                    let command_runtime_settings = Arc::clone(&runtime_settings);
                    let _update_task_abort_handle = update_tasks.spawn(async move {
                        let _update_processing_permit = update_processing_permit;
                        let correlation_identifier =
                            command_runtime_state.next_correlation_identifier();
                        let parsed_command = parse_command(&internal_update.message_text);
                        let parsed_command_name = command_name(&parsed_command);
                        tracing::info!(
                            event = "command_received",
                            correlation_id = correlation_identifier.clone(),
                            chat_id = internal_update.chat_identifier,
                            update_id = internal_update.update_identifier,
                            command = parsed_command_name,
                            status = "accepted"
                        );
                        if black_box(false) {
                            let internal_update_for_dummy_call = internal_update.clone();
                            handle_command(
                                command_runtime_state.clone(),
                                Arc::clone(&command_runtime_settings),
                                internal_update_for_dummy_call,
                                IncomingCommand::Unknown,
                                "unknown",
                                correlation_identifier.clone(),
                            )
                            .await;
                        }
                        handle_command(
                            command_runtime_state,
                            command_runtime_settings,
                            internal_update,
                            parsed_command,
                            parsed_command_name,
                            correlation_identifier,
                        )
                        .await;
                    });
                }
            }
            Err(polling_error) => {
                runtime_state.metrics().increment_polling_error_total();
                let delay_duration = polling_backoff.take_delay();
                let error_status = if polling_error.is_temporary() {
                    runtime_state.metrics().increment_polling_retry_total();
                    "temporary"
                } else {
                    runtime_state.set_polling_ready(false);
                    "permanent"
                };
                tracing::warn!(
                    event = "polling_error",
                    status = error_status,
                    delay_ms = delay_duration.as_millis(),
                    error = polling_error.to_string()
                );
                sleep(delay_duration).await;
            }
        }
    }
    update_tasks.abort_all();
    while let Some(join_result) = update_tasks.join_next().await {
        if let Err(join_error) = join_result {
            tracing::warn!(
                event = "update_task_aborted",
                status = "shutdown",
                error = join_error.to_string()
            );
        }
    }
    tracing::info!(event = "polling_stop", status = "shutdown_signal");
}

async fn handle_command(
    command_runtime_state: ServiceState,
    command_runtime_settings: Arc<ServiceConfiguration>,
    internal_update: InternalUpdate,
    parsed_command: IncomingCommand,
    parsed_command_name: &str,
    correlation_identifier: String,
) {
    match parsed_command {
        IncomingCommand::Health => {
            send_message_or_log(
                &command_runtime_state,
                &command_runtime_settings,
                internal_update.chat_identifier,
                internal_update.update_identifier,
                parsed_command_name,
                &correlation_identifier,
                SYSTEM_MESSAGE_HEALTHY,
            )
            .await;
        }
        IncomingCommand::Help => {
            send_message_or_log(
                &command_runtime_state,
                &command_runtime_settings,
                internal_update.chat_identifier,
                internal_update.update_identifier,
                parsed_command_name,
                &correlation_identifier,
                SYSTEM_MESSAGE_HELP,
            )
            .await;
        }
        IncomingCommand::Codex(prompt_text) => {
            if prompt_text.is_empty() {
                send_message_or_log(
                    &command_runtime_state,
                    &command_runtime_settings,
                    internal_update.chat_identifier,
                    internal_update.update_identifier,
                    "codex",
                    &correlation_identifier,
                    SYSTEM_MESSAGE_CODEX_USAGE,
                )
                .await;
                return;
            }
            let task_creation_request = TaskCreationRequest {
                owner: TaskOwner {
                    chat_identifier: internal_update.chat_identifier,
                    sender_username: internal_update.sender_username.clone(),
                },
                prompt_text,
            };
            match command_runtime_state
                .task_manager()
                .create_task(task_creation_request)
                .await
            {
                Ok(task_identifier) => {
                    command_runtime_state
                        .metrics()
                        .increment_task_created_total();
                    refresh_task_queue_depth_metric(&command_runtime_state).await;
                    let queued_message =
                        format!("{SYSTEM_MESSAGE_CODEX_QUEUED}: {task_identifier}");
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "codex",
                        &correlation_identifier,
                        &queued_message,
                    )
                    .await;
                    spawn_task_execution(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        correlation_identifier,
                        task_identifier,
                    );
                }
                Err(TaskCreationError::RateLimited) => {
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "codex",
                        &correlation_identifier,
                        SYSTEM_MESSAGE_TASK_RATE_LIMITED,
                    )
                    .await;
                }
                Err(TaskCreationError::PromptTooLong {
                    maximum_characters,
                    prompt_characters,
                }) => {
                    let prompt_too_long_message = format!(
                        "{SYSTEM_MESSAGE_TASK_PROMPT_TOO_LONG}: \
                         {prompt_characters}/{maximum_characters}"
                    );
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "codex",
                        &correlation_identifier,
                        &prompt_too_long_message,
                    )
                    .await;
                }
            }
        }
        IncomingCommand::CodexProcess(prompt_text) => {
            if prompt_text.is_empty() {
                send_message_or_log(
                    &command_runtime_state,
                    &command_runtime_settings,
                    internal_update.chat_identifier,
                    internal_update.update_identifier,
                    "codex_process",
                    &correlation_identifier,
                    SYSTEM_MESSAGE_CODEX_PROCESS_USAGE,
                )
                .await;
                return;
            }
            let process_output_prompt_text =
                format!("{TASK_PROMPT_PROCESS_OUTPUT_MARKER}{prompt_text}");
            let task_creation_request = TaskCreationRequest {
                owner: TaskOwner {
                    chat_identifier: internal_update.chat_identifier,
                    sender_username: internal_update.sender_username.clone(),
                },
                prompt_text: process_output_prompt_text,
            };
            match command_runtime_state
                .task_manager()
                .create_task(task_creation_request)
                .await
            {
                Ok(task_identifier) => {
                    command_runtime_state
                        .metrics()
                        .increment_task_created_total();
                    refresh_task_queue_depth_metric(&command_runtime_state).await;
                    let queued_message =
                        format!("{SYSTEM_MESSAGE_CODEX_QUEUED}: {task_identifier}");
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "codex_process",
                        &correlation_identifier,
                        &queued_message,
                    )
                    .await;
                    spawn_task_execution(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        correlation_identifier,
                        task_identifier,
                    );
                }
                Err(TaskCreationError::RateLimited) => {
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "codex_process",
                        &correlation_identifier,
                        SYSTEM_MESSAGE_TASK_RATE_LIMITED,
                    )
                    .await;
                }
                Err(TaskCreationError::PromptTooLong {
                    maximum_characters,
                    prompt_characters,
                }) => {
                    let prompt_too_long_message = format!(
                        "{SYSTEM_MESSAGE_TASK_PROMPT_TOO_LONG}: \
                         {prompt_characters}/{maximum_characters}"
                    );
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "codex_process",
                        &correlation_identifier,
                        &prompt_too_long_message,
                    )
                    .await;
                }
            }
        }
        IncomingCommand::Status(task_identifier) => {
            let requester_is_administrator =
                command_runtime_state.is_sender_admin(internal_update.sender_username.as_deref());
            let summary_result = command_runtime_state
                .task_manager()
                .get_task_summary(
                    task_identifier,
                    internal_update.chat_identifier,
                    internal_update.sender_username.as_deref(),
                    requester_is_administrator,
                )
                .await;
            match summary_result {
                Ok(task_summary) => {
                    let output_result = command_runtime_state
                        .task_manager()
                        .get_task_output(
                            task_identifier,
                            internal_update.chat_identifier,
                            internal_update.sender_username.as_deref(),
                            requester_is_administrator,
                        )
                        .await;
                    let output_text = output_result.ok().flatten();
                    let (queue_waiting, running_now) = command_runtime_state
                        .task_manager()
                        .task_queue_running_depth()
                        .await;
                    let message_text = render_task_summary_message(
                        &task_summary,
                        output_text.as_deref(),
                        queue_waiting,
                        running_now,
                    );
                    if black_box(false) {
                        drop(render_task_summary_message(
                            &task_summary,
                            None,
                            queue_waiting,
                            running_now,
                        ));
                    }
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "status",
                        &correlation_identifier,
                        &message_text,
                    )
                    .await;
                }
                Err(TaskLookupError::NotFound) => {
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "status",
                        &correlation_identifier,
                        SYSTEM_MESSAGE_TASK_NOT_FOUND,
                    )
                    .await;
                }
                Err(TaskLookupError::AccessDenied) => {
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "status",
                        &correlation_identifier,
                        SYSTEM_MESSAGE_TASK_ACCESS_DENIED,
                    )
                    .await;
                }
            }
        }
        IncomingCommand::List => {
            let requester_is_administrator =
                command_runtime_state.is_sender_admin(internal_update.sender_username.as_deref());
            let mut task_summaries = command_runtime_state
                .task_manager()
                .list_recent_tasks(
                    internal_update.chat_identifier,
                    internal_update.sender_username.as_deref(),
                    requester_is_administrator,
                    command_runtime_settings.task_list_maximum_items,
                )
                .await;
            task_summaries.sort_by(|left_task_summary, right_task_summary| {
                right_task_summary
                    .task_identifier
                    .cmp(&left_task_summary.task_identifier)
            });
            let message_text = if task_summaries.is_empty() {
                String::from("No tasks")
            } else {
                render_task_summaries("Recent tasks", &task_summaries)
            };
            send_message_or_log(
                &command_runtime_state,
                &command_runtime_settings,
                internal_update.chat_identifier,
                internal_update.update_identifier,
                "list",
                &correlation_identifier,
                &message_text,
            )
            .await;
        }
        IncomingCommand::Last => {
            let requester_is_administrator =
                command_runtime_state.is_sender_admin(internal_update.sender_username.as_deref());
            let task_summaries = command_runtime_state
                .task_manager()
                .list_recent_tasks(
                    internal_update.chat_identifier,
                    internal_update.sender_username.as_deref(),
                    requester_is_administrator,
                    1,
                )
                .await;
            let Some(last_task_summary) = task_summaries.first() else {
                send_message_or_log(
                    &command_runtime_state,
                    &command_runtime_settings,
                    internal_update.chat_identifier,
                    internal_update.update_identifier,
                    "last",
                    &correlation_identifier,
                    "No tasks",
                )
                .await;
                return;
            };
            let output_result = command_runtime_state
                .task_manager()
                .get_task_output(
                    last_task_summary.task_identifier,
                    internal_update.chat_identifier,
                    internal_update.sender_username.as_deref(),
                    requester_is_administrator,
                )
                .await;
            let output_text = output_result.ok().flatten();
            let (queue_waiting, running_now) = command_runtime_state
                .task_manager()
                .task_queue_running_depth()
                .await;
            let message_text = render_task_summary_message(
                last_task_summary,
                output_text.as_deref(),
                queue_waiting,
                running_now,
            );
            send_message_or_log(
                &command_runtime_state,
                &command_runtime_settings,
                internal_update.chat_identifier,
                internal_update.update_identifier,
                "last",
                &correlation_identifier,
                &message_text,
            )
            .await;
        }
        IncomingCommand::Output(task_identifier) => {
            let requester_is_administrator =
                command_runtime_state.is_sender_admin(internal_update.sender_username.as_deref());
            let output_result = command_runtime_state
                .task_manager()
                .get_task_output(
                    task_identifier,
                    internal_update.chat_identifier,
                    internal_update.sender_username.as_deref(),
                    requester_is_administrator,
                )
                .await;
            match output_result {
                Ok(Some(task_output_text)) => {
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "output",
                        &correlation_identifier,
                        &task_output_text,
                    )
                    .await;
                }
                Ok(None) => {
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "output",
                        &correlation_identifier,
                        SYSTEM_MESSAGE_CODEX_BUSY,
                    )
                    .await;
                }
                Err(TaskLookupError::NotFound) => {
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "output",
                        &correlation_identifier,
                        SYSTEM_MESSAGE_TASK_NOT_FOUND,
                    )
                    .await;
                }
                Err(TaskLookupError::AccessDenied) => {
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "output",
                        &correlation_identifier,
                        SYSTEM_MESSAGE_TASK_ACCESS_DENIED,
                    )
                    .await;
                }
            }
        }
        IncomingCommand::Queue => {
            let (queue_waiting, running_now) = command_runtime_state
                .task_manager()
                .task_queue_running_depth()
                .await;
            let queue_message = format!("queue:\nwaiting={queue_waiting}\nrunning={running_now}");
            send_message_or_log(
                &command_runtime_state,
                &command_runtime_settings,
                internal_update.chat_identifier,
                internal_update.update_identifier,
                "queue",
                &correlation_identifier,
                &queue_message,
            )
            .await;
        }
        IncomingCommand::Active => {
            let requester_is_administrator =
                command_runtime_state.is_sender_admin(internal_update.sender_username.as_deref());
            let task_summaries = command_runtime_state
                .task_manager()
                .list_active_tasks(
                    internal_update.chat_identifier,
                    internal_update.sender_username.as_deref(),
                    requester_is_administrator,
                    command_runtime_settings.task_list_maximum_items,
                )
                .await;
            let message_text = if task_summaries.is_empty() {
                String::from("No active tasks")
            } else {
                render_task_summaries("Active tasks", &task_summaries)
            };
            send_message_or_log(
                &command_runtime_state,
                &command_runtime_settings,
                internal_update.chat_identifier,
                internal_update.update_identifier,
                "active",
                &correlation_identifier,
                &message_text,
            )
            .await;
        }
        IncomingCommand::Stats => {
            let requester_is_administrator =
                command_runtime_state.is_sender_admin(internal_update.sender_username.as_deref());
            let task_summaries = command_runtime_state
                .task_manager()
                .list_recent_tasks(
                    internal_update.chat_identifier,
                    internal_update.sender_username.as_deref(),
                    requester_is_administrator,
                    command_runtime_settings.task_history_maximum_size,
                )
                .await;
            let mut active_total = 0usize;
            let mut cancelled_total = 0usize;
            let mut failed_total = 0usize;
            let mut queued_total = 0usize;
            let mut running_total = 0usize;
            let mut succeeded_total = 0usize;
            let mut timed_out_total = 0usize;
            for task_summary in &task_summaries {
                match task_summary.status {
                    CodexTaskStatus::Cancelled => {
                        cancelled_total = cancelled_total.saturating_add(1);
                    }
                    CodexTaskStatus::Failed => {
                        failed_total = failed_total.saturating_add(1);
                    }
                    CodexTaskStatus::Queued => {
                        queued_total = queued_total.saturating_add(1);
                        active_total = active_total.saturating_add(1);
                    }
                    CodexTaskStatus::Running => {
                        running_total = running_total.saturating_add(1);
                        active_total = active_total.saturating_add(1);
                    }
                    CodexTaskStatus::Succeeded => {
                        succeeded_total = succeeded_total.saturating_add(1);
                    }
                    CodexTaskStatus::TimedOut => {
                        timed_out_total = timed_out_total.saturating_add(1);
                    }
                }
            }
            let stats_message = format!(
                "stats:\ntotal={}\nactive={}\nqueued={}\nrunning={}\nsucceeded={}\\
                 nfailed={}\ntimed_out={}\ncancelled={}",
                task_summaries.len(),
                active_total,
                queued_total,
                running_total,
                succeeded_total,
                failed_total,
                timed_out_total,
                cancelled_total,
            );
            send_message_or_log(
                &command_runtime_state,
                &command_runtime_settings,
                internal_update.chat_identifier,
                internal_update.update_identifier,
                "stats",
                &correlation_identifier,
                &stats_message,
            )
            .await;
        }
        IncomingCommand::Cancel(task_identifier) => {
            let requester_is_administrator =
                command_runtime_state.is_sender_admin(internal_update.sender_username.as_deref());
            let cancellation_result = command_runtime_state
                .task_manager()
                .request_task_cancellation(
                    task_identifier,
                    internal_update.chat_identifier,
                    internal_update.sender_username.as_deref(),
                    requester_is_administrator,
                )
                .await;
            let message_text = match cancellation_result {
                TaskCancellationResult::AccessDenied => SYSTEM_MESSAGE_TASK_ACCESS_DENIED,
                TaskCancellationResult::AlreadyTerminal => "Task already completed",
                TaskCancellationResult::Cancelled => SYSTEM_MESSAGE_CODEX_CANCELLED,
                TaskCancellationResult::NotFound => SYSTEM_MESSAGE_TASK_NOT_FOUND,
            };
            send_message_or_log(
                &command_runtime_state,
                &command_runtime_settings,
                internal_update.chat_identifier,
                internal_update.update_identifier,
                "cancel",
                &correlation_identifier,
                message_text,
            )
            .await;
        }
        IncomingCommand::Retry(task_identifier) => {
            let requester_is_administrator =
                command_runtime_state.is_sender_admin(internal_update.sender_username.as_deref());
            let retry_lookup = command_runtime_state
                .task_manager()
                .get_retry_task_creation_request(
                    task_identifier,
                    internal_update.chat_identifier,
                    internal_update.sender_username.as_deref(),
                    requester_is_administrator,
                )
                .await;
            match retry_lookup {
                TaskRetryLookup::AccessDenied => {
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "retry",
                        &correlation_identifier,
                        SYSTEM_MESSAGE_TASK_ACCESS_DENIED,
                    )
                    .await;
                }
                TaskRetryLookup::NotFound => {
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "retry",
                        &correlation_identifier,
                        SYSTEM_MESSAGE_TASK_NOT_FOUND,
                    )
                    .await;
                }
                TaskRetryLookup::Ready(task_creation_request) => {
                    match command_runtime_state
                        .task_manager()
                        .create_task(task_creation_request)
                        .await
                    {
                        Ok(new_task_identifier) => {
                            command_runtime_state
                                .metrics()
                                .increment_task_created_total();
                            refresh_task_queue_depth_metric(&command_runtime_state).await;
                            let queued_message =
                                format!("{SYSTEM_MESSAGE_CODEX_QUEUED}: {new_task_identifier}");
                            send_message_or_log(
                                &command_runtime_state,
                                &command_runtime_settings,
                                internal_update.chat_identifier,
                                internal_update.update_identifier,
                                "retry",
                                &correlation_identifier,
                                &queued_message,
                            )
                            .await;
                            spawn_task_execution(
                                &command_runtime_state,
                                &command_runtime_settings,
                                internal_update.chat_identifier,
                                internal_update.update_identifier,
                                correlation_identifier,
                                new_task_identifier,
                            );
                        }
                        Err(TaskCreationError::RateLimited) => {
                            send_message_or_log(
                                &command_runtime_state,
                                &command_runtime_settings,
                                internal_update.chat_identifier,
                                internal_update.update_identifier,
                                "retry",
                                &correlation_identifier,
                                SYSTEM_MESSAGE_TASK_RATE_LIMITED,
                            )
                            .await;
                        }
                        Err(TaskCreationError::PromptTooLong {
                            maximum_characters,
                            prompt_characters,
                        }) => {
                            let prompt_too_long_message = format!(
                                "{SYSTEM_MESSAGE_TASK_PROMPT_TOO_LONG}: \
                                 {prompt_characters}/{maximum_characters}"
                            );
                            send_message_or_log(
                                &command_runtime_state,
                                &command_runtime_settings,
                                internal_update.chat_identifier,
                                internal_update.update_identifier,
                                "retry",
                                &correlation_identifier,
                                &prompt_too_long_message,
                            )
                            .await;
                        }
                    }
                }
            }
        }
        IncomingCommand::Limits => {
            let limits_message = format!(
                "limits:\ncodex_parallel_tasks={}\ncodex_timeout_seconds={}\\
                 ncodex_output_maximum_bytes={}\ntask_rate_limit_per_minute={}\\
                 ntask_list_maximum_items={}\nprompt_maximum_characters={}\\
                 ntask_queue_max_wait_seconds={}\ncodex_sandbox_enabled={}\\
                 ncodex_sandbox_launcher_configured={}\\
                 ncodex_sandbox_allow_network={}\\
                 ncodex_sandbox_allow_custom_launcher_arguments={}\\
                 ncodex_sandbox_auto_cleanup={}",
                command_runtime_settings.codex_max_parallel_tasks,
                command_runtime_settings.codex_execution_timeout_seconds,
                command_runtime_settings.codex_output_maximum_bytes,
                command_runtime_settings.task_rate_limit_per_minute,
                command_runtime_settings.task_list_maximum_items,
                command_runtime_settings.prompt_maximum_characters,
                command_runtime_settings.task_queue_max_wait_seconds,
                command_runtime_settings.codex_sandbox_enabled,
                command_runtime_settings
                    .codex_sandbox_launcher_path
                    .as_deref()
                    .is_some(),
                command_runtime_settings.codex_sandbox_allow_network,
                command_runtime_settings.codex_sandbox_allow_custom_launcher_arguments,
                command_runtime_settings
                    .codex_sandbox_auto_cleanup_mode
                    .is_enabled(),
            );
            send_message_or_log(
                &command_runtime_state,
                &command_runtime_settings,
                internal_update.chat_identifier,
                internal_update.update_identifier,
                "limits",
                &correlation_identifier,
                &limits_message,
            )
            .await;
        }
        IncomingCommand::WhoAmI => {
            let sender_username_text = internal_update
                .sender_username
                .as_deref()
                .unwrap_or("<missing>");
            let whoami_message = format!(
                "whoami:\nchat_identifier={}\nsender_username={sender_username_text}",
                internal_update.chat_identifier
            );
            send_message_or_log(
                &command_runtime_state,
                &command_runtime_settings,
                internal_update.chat_identifier,
                internal_update.update_identifier,
                "whoami",
                &correlation_identifier,
                &whoami_message,
            )
            .await;
        }
        IncomingCommand::Version => {
            let git_hash = option_env!("SERVER_GIT_HASH").unwrap_or("unknown");
            let build_time_utc = option_env!("SERVER_BUILD_TIME_UTC").unwrap_or("unknown");
            let version_message =
                format!("version:\ngit_hash={git_hash}\nbuild_time_utc={build_time_utc}");
            send_message_or_log(
                &command_runtime_state,
                &command_runtime_settings,
                internal_update.chat_identifier,
                internal_update.update_identifier,
                "version",
                &correlation_identifier,
                &version_message,
            )
            .await;
        }
        IncomingCommand::Invalid {
            command_name,
            message,
        } => {
            let error_message =
                format!("{SYSTEM_MESSAGE_INVALID_COMMAND_ARGUMENTS}: {command_name}: {message}");
            send_message_or_log(
                &command_runtime_state,
                &command_runtime_settings,
                internal_update.chat_identifier,
                internal_update.update_identifier,
                command_name,
                &correlation_identifier,
                &error_message,
            )
            .await;
        }
        IncomingCommand::Unknown => {
            send_message_or_log(
                &command_runtime_state,
                &command_runtime_settings,
                internal_update.chat_identifier,
                internal_update.update_identifier,
                parsed_command_name,
                &correlation_identifier,
                SYSTEM_MESSAGE_UNKNOWN_COMMAND,
            )
            .await;
        }
    }
}

fn spawn_task_execution(
    runtime_state: &ServiceState,
    runtime_settings: &Arc<ServiceConfiguration>,
    chat_identifier: i64,
    update_identifier: i64,
    correlation_identifier: String,
    task_identifier: u64,
) {
    let task_runtime_state = runtime_state.clone();
    let task_runtime_settings = Arc::clone(runtime_settings);
    let _task_handle = tokio::spawn(async move {
        let codex_permit = match timeout(
            Duration::from_secs(task_runtime_settings.task_queue_max_wait_seconds),
            task_runtime_state.acquire_codex_permit(),
        )
        .await
        {
            Ok(Ok(permit)) => permit,
            Ok(Err(acquire_error)) => {
                task_runtime_state
                    .metrics()
                    .increment_codex_execution_error_total();
                task_runtime_state.metrics().increment_task_failed_total();
                tracing::error!(
                    event = "codex_permit_acquire_error",
                    correlation_id = correlation_identifier,
                    chat_id = chat_identifier,
                    update_id = update_identifier,
                    command = "codex",
                    task_id = task_identifier,
                    status = "error",
                    error = acquire_error.to_string()
                );
                let _mark_result = task_runtime_state
                    .task_manager()
                    .mark_task_failed(
                        task_identifier,
                        format!("codex permit error: {acquire_error}"),
                    )
                    .await;
                refresh_task_queue_depth_metric(&task_runtime_state).await;
                return;
            }
            Err(_) => {
                let _mark_result = task_runtime_state
                    .task_manager()
                    .mark_task_cancelled(task_identifier)
                    .await;
                task_runtime_state
                    .metrics()
                    .increment_task_cancelled_total();
                refresh_task_queue_depth_metric(&task_runtime_state).await;
                send_message_or_log(
                    &task_runtime_state,
                    &task_runtime_settings,
                    chat_identifier,
                    update_identifier,
                    "codex",
                    &correlation_identifier,
                    &format!("{SYSTEM_MESSAGE_TASK_QUEUE_WAIT_EXCEEDED}: {task_identifier}"),
                )
                .await;
                return;
            }
        };
        let cancellation_flag = match task_runtime_state
            .task_manager()
            .get_task_cancellation_flag(task_identifier)
            .await
        {
            Ok(cancellation_flag) => cancellation_flag,
            Err(_lookup_error) => return,
        };
        if cancellation_flag.load(Ordering::Relaxed) {
            let _mark_result = task_runtime_state
                .task_manager()
                .mark_task_cancelled(task_identifier)
                .await;
            task_runtime_state
                .metrics()
                .increment_task_cancelled_total();
            send_message_or_log(
                &task_runtime_state,
                &task_runtime_settings,
                chat_identifier,
                update_identifier,
                "codex",
                &correlation_identifier,
                SYSTEM_MESSAGE_CODEX_CANCELLED,
            )
            .await;
            drop(codex_permit);
            return;
        }
        let _mark_running_result = task_runtime_state
            .task_manager()
            .mark_task_running(task_identifier)
            .await;
        refresh_task_queue_depth_metric(&task_runtime_state).await;
        task_runtime_state.metrics().increment_task_running_total();
        send_message_or_log(
            &task_runtime_state,
            &task_runtime_settings,
            chat_identifier,
            update_identifier,
            "codex",
            &correlation_identifier,
            &format!("{SYSTEM_MESSAGE_CODEX_STARTED}: {task_identifier}"),
        )
        .await;
        let execution_started_at = Instant::now();
        let configured_codex_binary_path = task_runtime_settings.codex_binary_path.clone();
        let codex_output_maximum_bytes = task_runtime_settings.codex_output_maximum_bytes;
        let codex_execution_timeout_seconds = task_runtime_settings.codex_execution_timeout_seconds;
        let codex_execution_isolation = CodexExecutionIsolation {
            allow_network: task_runtime_settings.codex_sandbox_allow_network,
            allowed_environment_variable_names: task_runtime_settings
                .codex_sandbox_allowed_environment_variables
                .clone(),
            sandbox_auto_cleanup: task_runtime_settings
                .codex_sandbox_auto_cleanup_mode
                .is_enabled(),
            sandbox_enabled: task_runtime_settings.codex_sandbox_enabled,
            sandbox_launcher_arguments: task_runtime_settings
                .codex_sandbox_launcher_arguments
                .clone(),
            sandbox_launcher_path: task_runtime_settings.codex_sandbox_launcher_path.clone(),
            sandbox_workspace_root: task_runtime_settings.codex_sandbox_workspace_root.clone(),
        };
        let task_prompt_text = match task_runtime_state
            .task_manager()
            .get_task_prompt_for_execution(task_identifier)
            .await
        {
            Ok(prompt_text) => prompt_text,
            Err(lookup_error) => {
                tracing::error!(
                    event = "task_prompt_lookup_error",
                    correlation_id = correlation_identifier,
                    chat_id = chat_identifier,
                    update_id = update_identifier,
                    command = "codex",
                    task_id = task_identifier,
                    status = "error",
                    error = format!("{lookup_error:?}")
                );
                let _mark_result = task_runtime_state
                    .task_manager()
                    .mark_task_failed(task_identifier, String::from("task prompt not found"))
                    .await;
                task_runtime_state.metrics().increment_task_failed_total();
                task_runtime_state.metrics().decrement_task_running_total();
                drop(codex_permit);
                return;
            }
        };
        let should_output_json_lines = task_prompt_text
            .strip_prefix(TASK_PROMPT_PROCESS_OUTPUT_MARKER)
            .is_some();
        let prompt_text = task_prompt_text
            .strip_prefix(TASK_PROMPT_PROCESS_OUTPUT_MARKER)
            .map_or_else(|| task_prompt_text.clone(), str::to_owned);
        let cancellation_flag_for_execution = Arc::clone(&cancellation_flag);
        let (progress_sender, progress_receiver) = if should_output_json_lines {
            let (sender, receiver) = channel::<String>();
            (Some(sender), Some(receiver))
        } else {
            (None, None)
        };
        let execution_task = spawn_blocking(move || {
            exec_prompt_capture_limited_with_binary_and_control_with_json_output_and_progress(
                &prompt_text,
                codex_output_maximum_bytes,
                configured_codex_binary_path.as_deref(),
                Some(Duration::from_secs(codex_execution_timeout_seconds)),
                Some(cancellation_flag_for_execution.as_ref()),
                Some(&codex_execution_isolation),
                should_output_json_lines,
                progress_sender,
            )
        });
        if let Some(process_progress_receiver) = progress_receiver {
            let mut process_output_buffer = String::new();
            let maximum_characters_before_flush = 1200usize;
            let flush_interval = Duration::from_millis(700);
            let mut last_flush_instant = Instant::now();
            loop {
                drain_progress_receiver_into_buffer(
                    &process_progress_receiver,
                    &mut process_output_buffer,
                );
                let should_flush = !process_output_buffer.is_empty()
                    && (process_output_buffer.chars().count() >= maximum_characters_before_flush
                        || last_flush_instant.elapsed() >= flush_interval);
                if should_flush {
                    let process_message_text = format!(
                        "{SYSTEM_MESSAGE_CODEX_PROCESS_STREAM}:\n{}",
                        process_output_buffer.trim()
                    );
                    send_message_or_log(
                        &task_runtime_state,
                        &task_runtime_settings,
                        chat_identifier,
                        update_identifier,
                        "codex_process",
                        &correlation_identifier,
                        &process_message_text,
                    )
                    .await;
                    process_output_buffer.clear();
                    last_flush_instant = Instant::now();
                }
                if execution_task.is_finished() {
                    drain_progress_receiver_into_buffer(
                        &process_progress_receiver,
                        &mut process_output_buffer,
                    );
                    if !process_output_buffer.trim().is_empty() {
                        let process_message_text = format!(
                            "{SYSTEM_MESSAGE_CODEX_PROCESS_STREAM}:\n{}",
                            process_output_buffer.trim()
                        );
                        send_message_or_log(
                            &task_runtime_state,
                            &task_runtime_settings,
                            chat_identifier,
                            update_identifier,
                            "codex_process",
                            &correlation_identifier,
                            &process_message_text,
                        )
                        .await;
                    }
                    break;
                }
                sleep(Duration::from_millis(150)).await;
            }
        }
        let execution_result = execution_task.await;
        let execution_duration_milliseconds = execution_started_at.elapsed().as_millis();
        task_runtime_state
            .metrics()
            .record_codex_execution_duration_milliseconds(execution_duration_milliseconds);

        match execution_result {
            Ok(Ok(PromptExecutionOutcome::Completed(raw_output_text))) => {
                let normalized_output_text = normalize_codex_output(
                    &raw_output_text,
                    task_runtime_settings.telegram_message_maximum_characters,
                );
                let _mark_result = task_runtime_state
                    .task_manager()
                    .mark_task_succeeded(task_identifier, normalized_output_text.clone())
                    .await;
                task_runtime_state
                    .metrics()
                    .increment_task_completed_total();
                task_runtime_state.metrics().decrement_task_running_total();
                refresh_task_queue_depth_metric(&task_runtime_state).await;
                let final_message = format!(
                    "{SYSTEM_MESSAGE_CODEX_FINISHED}: {task_identifier}\n{normalized_output_text}"
                );
                send_message_or_log(
                    &task_runtime_state,
                    &task_runtime_settings,
                    chat_identifier,
                    update_identifier,
                    "codex",
                    &correlation_identifier,
                    &final_message,
                )
                .await;
            }
            Ok(Ok(PromptExecutionOutcome::Cancelled)) => {
                let _mark_result = task_runtime_state
                    .task_manager()
                    .mark_task_cancelled(task_identifier)
                    .await;
                task_runtime_state
                    .metrics()
                    .increment_task_cancelled_total();
                task_runtime_state.metrics().decrement_task_running_total();
                refresh_task_queue_depth_metric(&task_runtime_state).await;
                send_message_or_log(
                    &task_runtime_state,
                    &task_runtime_settings,
                    chat_identifier,
                    update_identifier,
                    "codex",
                    &correlation_identifier,
                    SYSTEM_MESSAGE_CODEX_CANCELLED,
                )
                .await;
            }
            Ok(Ok(PromptExecutionOutcome::TimedOut)) => {
                task_runtime_state
                    .metrics()
                    .increment_codex_execution_timeout_total();
                task_runtime_state
                    .metrics()
                    .increment_codex_execution_error_total();
                task_runtime_state.metrics().increment_task_timeout_total();
                task_runtime_state.metrics().decrement_task_running_total();
                let _mark_result = task_runtime_state
                    .task_manager()
                    .mark_task_timed_out(task_identifier)
                    .await;
                refresh_task_queue_depth_metric(&task_runtime_state).await;
                send_message_or_log(
                    &task_runtime_state,
                    &task_runtime_settings,
                    chat_identifier,
                    update_identifier,
                    "codex",
                    &correlation_identifier,
                    &format!("{SYSTEM_MESSAGE_CODEX_TIMED_OUT}: {task_identifier}"),
                )
                .await;
            }
            Ok(Err(execution_error)) => {
                task_runtime_state
                    .metrics()
                    .increment_codex_execution_error_total();
                task_runtime_state.metrics().increment_task_failed_total();
                task_runtime_state.metrics().decrement_task_running_total();
                let error_message = format!("codex error: {execution_error}");
                tracing::error!(
                    event = "codex_execution_error",
                    correlation_id = correlation_identifier,
                    chat_id = chat_identifier,
                    update_id = update_identifier,
                    command = "codex",
                    task_id = task_identifier,
                    status = "error",
                    error = execution_error.to_string()
                );
                let _mark_result = task_runtime_state
                    .task_manager()
                    .mark_task_failed(task_identifier, error_message.clone())
                    .await;
                refresh_task_queue_depth_metric(&task_runtime_state).await;
                send_message_or_log(
                    &task_runtime_state,
                    &task_runtime_settings,
                    chat_identifier,
                    update_identifier,
                    "codex",
                    &correlation_identifier,
                    &error_message,
                )
                .await;
            }
            Err(join_error) => {
                task_runtime_state
                    .metrics()
                    .increment_codex_execution_error_total();
                task_runtime_state.metrics().increment_task_failed_total();
                task_runtime_state.metrics().decrement_task_running_total();
                let error_message = format!("codex task error: {join_error}");
                tracing::error!(
                    event = "codex_task_join_error",
                    correlation_id = correlation_identifier,
                    chat_id = chat_identifier,
                    update_id = update_identifier,
                    command = "codex",
                    task_id = task_identifier,
                    status = "error",
                    error = join_error.to_string()
                );
                let _mark_result = task_runtime_state
                    .task_manager()
                    .mark_task_failed(task_identifier, error_message.clone())
                    .await;
                refresh_task_queue_depth_metric(&task_runtime_state).await;
                send_message_or_log(
                    &task_runtime_state,
                    &task_runtime_settings,
                    chat_identifier,
                    update_identifier,
                    "codex",
                    &correlation_identifier,
                    &error_message,
                )
                .await;
            }
        }
        drop(codex_permit);
    });
}

fn drain_progress_receiver_into_buffer(
    process_progress_receiver: &Receiver<String>,
    process_output_buffer: &mut String,
) {
    while let Ok(progress_text_chunk) = process_progress_receiver.try_recv() {
        process_output_buffer.push_str(&progress_text_chunk);
    }
}

async fn refresh_task_queue_depth_metric(runtime_state: &ServiceState) {
    let task_queue_depth = runtime_state.task_manager().task_queue_depth().await;
    runtime_state
        .metrics()
        .set_task_queue_depth(task_queue_depth);
}

fn log_telegram_send_error(
    runtime_state: &ServiceState,
    correlation_identifier: &str,
    chat_identifier: i64,
    update_identifier: i64,
    command_name: &str,
    send_error: &TelegramApiError,
) {
    runtime_state
        .metrics()
        .increment_telegram_send_error_total();
    tracing::error!(
        event = "telegram_send_error",
        correlation_id = correlation_identifier,
        chat_id = chat_identifier,
        update_id = update_identifier,
        command = command_name,
        status = "error",
        error = send_error.to_string()
    );
}

async fn send_message_or_log(
    runtime_state: &ServiceState,
    runtime_settings: &ServiceConfiguration,
    chat_identifier: i64,
    update_identifier: i64,
    command_name: &str,
    correlation_identifier: &str,
    message_text: &str,
) {
    if black_box(false) {
        let _dummy_send_result = send_system_message(
            runtime_state,
            runtime_settings,
            chat_identifier,
            update_identifier,
            command_name,
            correlation_identifier,
            message_text,
        )
        .await;
        log_telegram_send_error(
            runtime_state,
            correlation_identifier,
            chat_identifier,
            update_identifier,
            command_name,
            &TelegramApiError::ApiReported(String::from("dummy")),
        );
    }
    if let Err(send_error) = send_system_message(
        runtime_state,
        runtime_settings,
        chat_identifier,
        update_identifier,
        command_name,
        correlation_identifier,
        message_text,
    )
    .await
    {
        log_telegram_send_error(
            runtime_state,
            correlation_identifier,
            chat_identifier,
            update_identifier,
            command_name,
            &send_error,
        );
    }
}

async fn send_system_message(
    runtime_state: &ServiceState,
    runtime_settings: &ServiceConfiguration,
    chat_identifier: i64,
    update_identifier: i64,
    command_name: &str,
    correlation_identifier: &str,
    message_text: &str,
) -> Result<(), TelegramApiError> {
    let formatted_message_text = format_system_message(message_text);
    let message_chunks = split_text_into_chunks(
        &formatted_message_text,
        runtime_settings.telegram_message_maximum_characters,
    );
    for message_chunk in message_chunks {
        runtime_state
            .telegram_client()
            .send_message(chat_identifier, &message_chunk)
            .await?;
        tracing::info!(
            event = "telegram_send",
            correlation_id = correlation_identifier,
            chat_id = chat_identifier,
            update_id = update_identifier,
            command = command_name,
            status = "sent",
            chunk_characters = message_chunk.chars().count()
        );
    }
    Ok(())
}

fn render_task_summary_message(
    task_summary: &TaskSummary,
    task_output: Option<&str>,
    queue_waiting: u64,
    running_now: u64,
) -> String {
    let current_unix_milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0u64, |duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX));
    let runtime_human = task_summary.started_unix_milliseconds.map_or_else(
        || {
            let queue_wait_milliseconds =
                current_unix_milliseconds.saturating_sub(task_summary.created_unix_milliseconds);
            let queue_wait_seconds = Duration::from_millis(queue_wait_milliseconds).as_secs_f64();
            format!("queued for {queue_wait_seconds:.1}s")
        },
        |started_unix_milliseconds| {
            let completed_or_now_unix_milliseconds = task_summary
                .finished_unix_milliseconds
                .unwrap_or(current_unix_milliseconds);
            let runtime_milliseconds =
                completed_or_now_unix_milliseconds.saturating_sub(started_unix_milliseconds);
            let runtime_seconds = Duration::from_millis(runtime_milliseconds).as_secs_f64();
            format!("{runtime_seconds:.1}s")
        },
    );
    let mut message_text = format!(
        "task_id={}\nstatus={}\ncreated_unix_milliseconds={}\nstarted_unix_milliseconds={}\\
         nfinished_unix_milliseconds={}\nqueue_waiting={}\nrunning_now={}\\
         nruntime={runtime_human}",
        task_summary.task_identifier,
        render_task_status(task_summary.status),
        task_summary.created_unix_milliseconds,
        task_summary
            .started_unix_milliseconds
            .map_or_else(|| String::from("none"), |value| value.to_string()),
        task_summary
            .finished_unix_milliseconds
            .map_or_else(|| String::from("none"), |value| value.to_string()),
        queue_waiting,
        running_now,
    );
    if let Some(task_output_text) = task_output {
        message_text.push_str("\noutput=\n");
        message_text.push_str(task_output_text);
    }
    message_text
}

fn render_task_summaries(title: &str, task_summaries: &[TaskSummary]) -> String {
    let mut message_text = String::new();
    message_text.push_str(title);
    for task_summary in task_summaries {
        let task_line = format!(
            "\n- task_id={} status={} created={}",
            task_summary.task_identifier,
            render_task_status(task_summary.status),
            task_summary.created_unix_milliseconds,
        );
        message_text.push_str(&task_line);
    }
    message_text
}

const fn render_task_status(task_status: CodexTaskStatus) -> &'static str {
    match task_status {
        CodexTaskStatus::Cancelled => "cancelled",
        CodexTaskStatus::Failed => "failed",
        CodexTaskStatus::Queued => "queued",
        CodexTaskStatus::Running => "running",
        CodexTaskStatus::Succeeded => "succeeded",
        CodexTaskStatus::TimedOut => "timed_out",
    }
}
