mod worker_message_delivery;
mod worker_message_formatting;
mod worker_state_helpers;

use std::{
    collections::{HashSet, VecDeque},
    fmt::Write as _,
    hint::black_box,
    process::Command,
    sync::{Arc, atomic::Ordering, mpsc::channel},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use codex_command_runtime::{
    CodexExecutionIsolation, PromptExecutionOutcome,
    exec_prompt_capture_limited_with_binary_and_control_with_json_output_and_progress,
};
use openai_command_runtime::{
    OpenaiExecutionConfiguration, exec_prompt_with_configuration_and_usage,
};
use tokio::{
    sync::watch,
    task::{JoinSet, spawn_blocking},
    time::{Instant, sleep, timeout},
};

use self::{
    worker_message_delivery::send_message_or_log,
    worker_message_formatting::{render_task_summaries, render_task_summary_message},
    worker_state_helpers::{drain_progress_receiver_into_buffer, refresh_task_queue_depth_metric},
};
use crate::{
    runtime::ServiceState,
    settings::{CodexBinaryPath, ServiceConfiguration},
    shared::{
        ChatIdentifier, CodexTaskStatus, CorrelationIdentifier,
        ERROR_MESSAGE_CODEX_EXECUTION_PREFIX, ERROR_MESSAGE_CODEX_PERMIT_PREFIX,
        ERROR_MESSAGE_CODEX_TASK_JOIN_PREFIX, ERROR_MESSAGE_SEMAPHORE_CLOSED,
        ERROR_MESSAGE_TASK_PROMPT_NOT_FOUND, IncomingCommand, IncomingCommandName, PromptText,
        SYSTEM_MESSAGE_CODEX_BUSY, SYSTEM_MESSAGE_CODEX_CANCELLED,
        SYSTEM_MESSAGE_CODEX_COMMAND_TIMED_OUT, SYSTEM_MESSAGE_CODEX_FINISHED,
        SYSTEM_MESSAGE_CODEX_PROCESS_USAGE, SYSTEM_MESSAGE_CODEX_QUEUED,
        SYSTEM_MESSAGE_CODEX_STARTED, SYSTEM_MESSAGE_CODEX_TIMED_OUT, SYSTEM_MESSAGE_CODEX_USAGE,
        SYSTEM_MESSAGE_DEBUG_USAGE, SYSTEM_MESSAGE_FEATURES_USAGE, SYSTEM_MESSAGE_HEALTHY,
        SYSTEM_MESSAGE_HELP, SYSTEM_MESSAGE_INVALID_COMMAND_ARGUMENTS,
        SYSTEM_MESSAGE_NO_ACTIVE_TASKS, SYSTEM_MESSAGE_NO_TASKS,
        SYSTEM_MESSAGE_OPENAI_NOT_CONFIGURED, SYSTEM_MESSAGE_OPENAI_TIMED_OUT,
        SYSTEM_MESSAGE_OPENAI_URLS_EMPTY, SYSTEM_MESSAGE_OPENAI_USAGE,
        SYSTEM_MESSAGE_SANDBOX_USAGE, SYSTEM_MESSAGE_TASK_ACCESS_DENIED,
        SYSTEM_MESSAGE_TASK_NOT_FOUND, SYSTEM_MESSAGE_TASK_PROMPT_TOO_LONG,
        SYSTEM_MESSAGE_TASK_QUEUE_WAIT_EXCEEDED, SYSTEM_MESSAGE_TASK_RATE_LIMITED,
        SYSTEM_MESSAGE_UNKNOWN_COMMAND, SYSTEM_MESSAGE_USERNAME_REQUIRED, TaskCreationRequest,
        TaskExecutionOutputText, TaskIdentifier, TaskOwner, UpdateIdentifier,
        append_task_completion_status_prompt_suffix, normalize_codex_output,
    },
    task_manager::{TaskCancellationResult, TaskCreationError, TaskLookupError, TaskRetryLookup},
    telegram::{
        commands::{command_name, parse_command},
        model::{InternalUpdate, convert_telegram_update_to_internal},
    },
};

const TASK_PROMPT_PROCESS_OUTPUT_MARKER: &str = "__task_prompt_process_output__: ";
const SYSTEM_MESSAGE_CODEX_PROCESS_STREAM: &str = "Codex process output";

struct CodexCommandExecutionResult {
    output_text: String,
    status_code: Option<i32>,
    succeeded: bool,
}

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
    insertion_order: VecDeque<UpdateIdentifier>,
    known_identifiers: HashSet<UpdateIdentifier>,
    maximum_size: usize,
}

impl ProcessedUpdateCache {
    fn contains(&self, update_identifier: UpdateIdentifier) -> bool {
        self.known_identifiers.contains(&update_identifier)
    }

    fn insert(&mut self, update_identifier: UpdateIdentifier) {
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
                UpdateIdentifier::from(0),
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
                    if telegram_update.update_identifier.as_i64() >= update_offset {
                        update_offset =
                            telegram_update.update_identifier.as_i64().saturating_add(1);
                    }
                    if processed_update_cache.contains(telegram_update.update_identifier) {
                        runtime_state.metrics().increment_update_duplicate_total();
                        tracing::info!(
                            event = "update_duplicate",
                            update_identifier = telegram_update.update_identifier.as_i64(),
                            status = "skipped"
                        );
                        continue;
                    }
                    processed_update_cache.insert(telegram_update.update_identifier);
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
                            chat_identifier = internal_update.chat_identifier.as_i64(),
                            update_identifier = internal_update.update_identifier.as_i64(),
                            status = "ignored"
                        );
                        continue;
                    }
                    if !runtime_state.is_update_authorized(
                        internal_update.chat_identifier,
                        internal_update.sender_username.as_ref(),
                    ) {
                        tracing::warn!(
                            event = "update_not_authorized",
                            chat_identifier = internal_update.chat_identifier.as_i64(),
                            sender_username = internal_update
                                .sender_username
                                .as_deref()
                                .map_or("<missing>", |sender_username| sender_username),
                            update_identifier = internal_update.update_identifier.as_i64(),
                            status = "ignored"
                        );
                        continue;
                    }
                    let update_processing_permit = loop {
                        if *shutdown_receiver.borrow() {
                            break 'polling_loop;
                        }
                        let acquire_result = timeout(
                            Duration::from_millis(250),
                            runtime_state.acquire_update_processing_permit(),
                        )
                        .await;
                        match acquire_result {
                            Ok(Ok(permit)) => break permit,
                            Ok(Err(_acquire_error)) => {
                                tracing::error!(
                                    event = "update_semaphore_error",
                                    status = "error",
                                    error = String::from(ERROR_MESSAGE_SEMAPHORE_CLOSED)
                                );
                                continue 'polling_loop;
                            }
                            Err(_elapsed) => {}
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
                            correlation_identifier = correlation_identifier.as_str(),
                            chat_identifier = internal_update.chat_identifier.as_i64(),
                            update_identifier = internal_update.update_identifier.as_i64(),
                            command = parsed_command_name.as_str(),
                            status = "accepted"
                        );
                        if black_box(false) {
                            let internal_update_for_dummy_call = internal_update.clone();
                            handle_command(
                                command_runtime_state.clone(),
                                Arc::clone(&command_runtime_settings),
                                internal_update_for_dummy_call,
                                IncomingCommand::Unknown,
                                IncomingCommandName::new("unknown"),
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
    parsed_command_name: IncomingCommandName,
    correlation_identifier: CorrelationIdentifier,
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
            let augmented_prompt_text =
                append_task_completion_status_prompt_suffix(prompt_text.as_str());
            let task_creation_request = TaskCreationRequest {
                owner: TaskOwner {
                    chat_identifier: internal_update.chat_identifier,
                    sender_username: internal_update.sender_username.clone(),
                },
                prompt_text: PromptText::from(augmented_prompt_text),
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
            let augmented_prompt_text =
                append_task_completion_status_prompt_suffix(prompt_text.as_str());
            let process_output_prompt_text =
                format!("{TASK_PROMPT_PROCESS_OUTPUT_MARKER}{augmented_prompt_text}");
            let task_creation_request = TaskCreationRequest {
                owner: TaskOwner {
                    chat_identifier: internal_update.chat_identifier,
                    sender_username: internal_update.sender_username.clone(),
                },
                prompt_text: PromptText::from(process_output_prompt_text),
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
        IncomingCommand::Openai(prompt_text) => {
            if prompt_text.is_empty() {
                send_message_or_log(
                    &command_runtime_state,
                    &command_runtime_settings,
                    internal_update.chat_identifier,
                    internal_update.update_identifier,
                    "openai",
                    &correlation_identifier,
                    SYSTEM_MESSAGE_OPENAI_USAGE,
                )
                .await;
                return;
            }
            let invalid_selector_message =
                "OpenAI selector must be a positive integer (1-based index)";
            let openai_prompt_text = prompt_text.as_str().trim();
            let mut remaining_arguments = openai_prompt_text;
            let mut selected_openai_configuration_index = 1usize;
            loop {
                if let Some(arguments_without_configuration_selector) =
                    remaining_arguments.strip_prefix("--configuration ")
                {
                    let (configuration_index_text, remaining_after_configuration_selector) =
                        arguments_without_configuration_selector
                            .split_once(char::is_whitespace)
                            .map_or(
                                (arguments_without_configuration_selector, ""),
                                |(left_part, right_part)| (left_part, right_part.trim_start()),
                            );
                    let parsed_configuration_index = configuration_index_text.parse::<usize>();
                    let Ok(parsed_configuration_index_value) = parsed_configuration_index else {
                        send_message_or_log(
                            &command_runtime_state,
                            &command_runtime_settings,
                            internal_update.chat_identifier,
                            internal_update.update_identifier,
                            "openai",
                            &correlation_identifier,
                            invalid_selector_message,
                        )
                        .await;
                        return;
                    };
                    if parsed_configuration_index_value == 0 {
                        send_message_or_log(
                            &command_runtime_state,
                            &command_runtime_settings,
                            internal_update.chat_identifier,
                            internal_update.update_identifier,
                            "openai",
                            &correlation_identifier,
                            invalid_selector_message,
                        )
                        .await;
                        return;
                    }
                    selected_openai_configuration_index = parsed_configuration_index_value;
                    remaining_arguments = remaining_after_configuration_selector;
                    continue;
                }
                break;
            }
            if command_runtime_settings.openai_configurations.is_empty() {
                send_message_or_log(
                    &command_runtime_state,
                    &command_runtime_settings,
                    internal_update.chat_identifier,
                    internal_update.update_identifier,
                    "openai",
                    &correlation_identifier,
                    SYSTEM_MESSAGE_OPENAI_NOT_CONFIGURED,
                )
                .await;
                return;
            }
            let selected_openai_configuration_index_zero_based =
                selected_openai_configuration_index.saturating_sub(1);
            let selected_openai_configuration = command_runtime_settings
                .openai_configurations
                .get(selected_openai_configuration_index_zero_based);
            let Some(selected_openai_configuration_value) = selected_openai_configuration else {
                let selection_message = format!(
                    "OpenAI configuration index out of range: \
                     {selected_openai_configuration_index}. available count: {}",
                    command_runtime_settings.openai_configurations.len()
                );
                send_message_or_log(
                    &command_runtime_state,
                    &command_runtime_settings,
                    internal_update.chat_identifier,
                    internal_update.update_identifier,
                    "openai",
                    &correlation_identifier,
                    &selection_message,
                )
                .await;
                return;
            };
            let openai_prompt_parts = remaining_arguments.split_once("||").map(
                |(system_prompt_part, user_prompt_part)| {
                    (system_prompt_part.trim(), user_prompt_part.trim())
                },
            );
            let openai_system_prompt =
                openai_prompt_parts.and_then(|(system_prompt_part, _user_prompt_part)| {
                    if system_prompt_part.is_empty() {
                        None
                    } else {
                        Some(system_prompt_part)
                    }
                });
            let openai_user_prompt_text = openai_prompt_parts.map_or_else(
                || remaining_arguments.trim(),
                |openai_prompt_parts_split| openai_prompt_parts_split.1,
            );
            if openai_user_prompt_text.is_empty() {
                send_message_or_log(
                    &command_runtime_state,
                    &command_runtime_settings,
                    internal_update.chat_identifier,
                    internal_update.update_identifier,
                    "openai",
                    &correlation_identifier,
                    SYSTEM_MESSAGE_OPENAI_USAGE,
                )
                .await;
                return;
            }
            let augmented_openai_user_prompt_text =
                append_task_completion_status_prompt_suffix(openai_user_prompt_text);
            let prompt_character_count = augmented_openai_user_prompt_text.chars().count();
            if prompt_character_count > command_runtime_settings.prompt_maximum_characters {
                let prompt_too_long_message = format!(
                    "{SYSTEM_MESSAGE_TASK_PROMPT_TOO_LONG}: {prompt_character_count}/{}",
                    command_runtime_settings.prompt_maximum_characters
                );
                send_message_or_log(
                    &command_runtime_state,
                    &command_runtime_settings,
                    internal_update.chat_identifier,
                    internal_update.update_identifier,
                    "openai",
                    &correlation_identifier,
                    &prompt_too_long_message,
                )
                .await;
                return;
            }
            let openai_execution_configuration = OpenaiExecutionConfiguration {
                application_programming_interface_key: selected_openai_configuration_value
                    .application_programming_interface_key
                    .as_str(),
                application_programming_interface_uniform_resource_locator:
                    selected_openai_configuration_value
                        .application_programming_interface_uniform_resource_locator
                        .as_str(),
                model: selected_openai_configuration_value.model.as_str(),
                system_prompt: openai_system_prompt,
            };
            tracing::info!(
                event = "openai_request_start",
                correlation_identifier = correlation_identifier.as_str(),
                chat_identifier = internal_update.chat_identifier.as_i64(),
                update_identifier = internal_update.update_identifier.as_i64(),
                configuration_index = selected_openai_configuration_index,
                model = selected_openai_configuration_value.model.as_str(),
                api_url = selected_openai_configuration_value
                    .application_programming_interface_uniform_resource_locator
                    .as_str(),
                prompt_characters = augmented_openai_user_prompt_text.chars().count(),
                has_system_prompt = openai_system_prompt.is_some(),
                status = "started"
            );
            let openai_execution_result = timeout(
                Duration::from_secs(command_runtime_settings.codex_execution_timeout_seconds),
                exec_prompt_with_configuration_and_usage(
                    augmented_openai_user_prompt_text.as_str(),
                    openai_execution_configuration,
                ),
            )
            .await;
            match openai_execution_result {
                Ok(Ok(openai_execution_result_with_usage)) => {
                    let usage_value = openai_execution_result_with_usage.usage;
                    tracing::info!(
                        event = "openai_request_usage",
                        correlation_identifier = correlation_identifier.as_str(),
                        chat_identifier = internal_update.chat_identifier.as_i64(),
                        update_identifier = internal_update.update_identifier.as_i64(),
                        configuration_index = selected_openai_configuration_index,
                        prompt_tokens = usage_value.and_then(|usage| usage.prompt_tokens),
                        completion_tokens = usage_value.and_then(|usage| usage.completion_tokens),
                        total_tokens = usage_value.and_then(|usage| usage.total_tokens),
                        status = "ok"
                    );
                    let normalized_output_text = normalize_codex_output(
                        openai_execution_result_with_usage.completion_text.as_str(),
                        command_runtime_settings.telegram_message_maximum_characters,
                    );
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "openai",
                        &correlation_identifier,
                        &normalized_output_text,
                    )
                    .await;
                }
                Ok(Err(execution_error)) => {
                    let error_message = format!("openai error: {execution_error}");
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "openai",
                        &correlation_identifier,
                        &error_message,
                    )
                    .await;
                }
                Err(_) => {
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "openai",
                        &correlation_identifier,
                        SYSTEM_MESSAGE_OPENAI_TIMED_OUT,
                    )
                    .await;
                }
            }
        }
        IncomingCommand::OpenaiUrls => {
            if command_runtime_settings.openai_configurations.is_empty() {
                send_message_or_log(
                    &command_runtime_state,
                    &command_runtime_settings,
                    internal_update.chat_identifier,
                    internal_update.update_identifier,
                    "openai_urls",
                    &correlation_identifier,
                    SYSTEM_MESSAGE_OPENAI_URLS_EMPTY,
                )
                .await;
                return;
            }
            let mut openai_uniform_resource_locators_message = String::from("openai_api_urls:");
            for (index, openai_configuration) in command_runtime_settings
                .openai_configurations
                .iter()
                .enumerate()
            {
                let index_one_based = index.saturating_add(1);
                let _write_result = writeln!(
                    &mut openai_uniform_resource_locators_message,
                    "{index_one_based}. {} (model: {})",
                    openai_configuration
                        .application_programming_interface_uniform_resource_locator
                        .as_str(),
                    openai_configuration.model.as_str()
                );
            }
            send_message_or_log(
                &command_runtime_state,
                &command_runtime_settings,
                internal_update.chat_identifier,
                internal_update.update_identifier,
                "openai_urls",
                &correlation_identifier,
                &openai_uniform_resource_locators_message,
            )
            .await;
        }
        IncomingCommand::CodexSandbox(command_arguments) => {
            if command_arguments.is_empty() {
                send_message_or_log(
                    &command_runtime_state,
                    &command_runtime_settings,
                    internal_update.chat_identifier,
                    internal_update.update_identifier,
                    "sandbox",
                    &correlation_identifier,
                    SYSTEM_MESSAGE_SANDBOX_USAGE,
                )
                .await;
                return;
            }
            let parsed_command_arguments = parse_command_line_arguments(command_arguments.as_str())
                .map_err(|parse_error| {
                    format!("{SYSTEM_MESSAGE_INVALID_COMMAND_ARGUMENTS}: sandbox: {parse_error}")
                });
            let parsed_arguments = match parsed_command_arguments {
                Ok(command_line_arguments) => command_line_arguments,
                Err(error_message) => {
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "sandbox",
                        &correlation_identifier,
                        &error_message,
                    )
                    .await;
                    return;
                }
            };
            let mut command_line_arguments = vec![String::from("sandbox")];
            command_line_arguments.extend(parsed_arguments);
            execute_codex_command_and_send_output(
                &command_runtime_state,
                &command_runtime_settings,
                &internal_update,
                &correlation_identifier,
                IncomingCommandName::new("sandbox"),
                command_line_arguments,
                Some(String::from(SYSTEM_MESSAGE_SANDBOX_USAGE)),
            )
            .await;
        }
        IncomingCommand::CodexDebug(command_arguments) => {
            if command_arguments.is_empty() {
                send_message_or_log(
                    &command_runtime_state,
                    &command_runtime_settings,
                    internal_update.chat_identifier,
                    internal_update.update_identifier,
                    "debug",
                    &correlation_identifier,
                    SYSTEM_MESSAGE_DEBUG_USAGE,
                )
                .await;
                return;
            }
            let parsed_command_arguments = parse_command_line_arguments(command_arguments.as_str())
                .map_err(|parse_error| {
                    format!("{SYSTEM_MESSAGE_INVALID_COMMAND_ARGUMENTS}: debug: {parse_error}")
                });
            let parsed_arguments = match parsed_command_arguments {
                Ok(command_line_arguments) => command_line_arguments,
                Err(error_message) => {
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "debug",
                        &correlation_identifier,
                        &error_message,
                    )
                    .await;
                    return;
                }
            };
            let mut command_line_arguments = vec![String::from("debug")];
            command_line_arguments.extend(parsed_arguments);
            execute_codex_command_and_send_output(
                &command_runtime_state,
                &command_runtime_settings,
                &internal_update,
                &correlation_identifier,
                IncomingCommandName::new("debug"),
                command_line_arguments,
                Some(String::from(SYSTEM_MESSAGE_DEBUG_USAGE)),
            )
            .await;
        }
        IncomingCommand::CodexFeatures(command_arguments) => {
            if command_arguments.is_empty() {
                send_message_or_log(
                    &command_runtime_state,
                    &command_runtime_settings,
                    internal_update.chat_identifier,
                    internal_update.update_identifier,
                    "features",
                    &correlation_identifier,
                    SYSTEM_MESSAGE_FEATURES_USAGE,
                )
                .await;
                return;
            }
            let parsed_command_arguments = parse_command_line_arguments(command_arguments.as_str())
                .map_err(|parse_error| {
                    format!("{SYSTEM_MESSAGE_INVALID_COMMAND_ARGUMENTS}: features: {parse_error}")
                });
            let parsed_arguments = match parsed_command_arguments {
                Ok(command_line_arguments) => command_line_arguments,
                Err(error_message) => {
                    send_message_or_log(
                        &command_runtime_state,
                        &command_runtime_settings,
                        internal_update.chat_identifier,
                        internal_update.update_identifier,
                        "features",
                        &correlation_identifier,
                        &error_message,
                    )
                    .await;
                    return;
                }
            };
            let mut command_line_arguments = vec![String::from("features")];
            command_line_arguments.extend(parsed_arguments);
            execute_codex_command_and_send_output(
                &command_runtime_state,
                &command_runtime_settings,
                &internal_update,
                &correlation_identifier,
                IncomingCommandName::new("features"),
                command_line_arguments,
                Some(String::from(SYSTEM_MESSAGE_FEATURES_USAGE)),
            )
            .await;
        }
        IncomingCommand::CodexMcpList => {
            execute_codex_command_and_send_output(
                &command_runtime_state,
                &command_runtime_settings,
                &internal_update,
                &correlation_identifier,
                IncomingCommandName::new("mcp_list"),
                vec![String::from("mcp"), String::from("list")],
                None,
            )
            .await;
        }
        IncomingCommand::CodexDebugPromptInput(prompt_text) => {
            let mut command_line_arguments =
                vec![String::from("debug"), String::from("prompt-input")];
            if !prompt_text.is_empty() {
                let augmented_prompt_text =
                    append_task_completion_status_prompt_suffix(prompt_text.as_str());
                command_line_arguments.push(augmented_prompt_text);
            }
            execute_codex_command_and_send_output(
                &command_runtime_state,
                &command_runtime_settings,
                &internal_update,
                &correlation_identifier,
                IncomingCommandName::new("debug_prompt_input"),
                command_line_arguments,
                None,
            )
            .await;
        }
        IncomingCommand::CodexFeaturesList => {
            execute_codex_command_and_send_output(
                &command_runtime_state,
                &command_runtime_settings,
                &internal_update,
                &correlation_identifier,
                IncomingCommandName::new("features_list"),
                vec![String::from("features"), String::from("list")],
                None,
            )
            .await;
        }
        IncomingCommand::Status(task_identifier) => {
            let requester_is_administrator =
                command_runtime_state.is_sender_admin(internal_update.sender_username.as_ref());
            let summary_result = command_runtime_state
                .task_manager()
                .get_task_summary(
                    task_identifier,
                    internal_update.chat_identifier,
                    internal_update.sender_username.as_ref(),
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
                            internal_update.sender_username.as_ref(),
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
                command_runtime_state.is_sender_admin(internal_update.sender_username.as_ref());
            let mut task_summaries = command_runtime_state
                .task_manager()
                .list_recent_tasks(
                    internal_update.chat_identifier,
                    internal_update.sender_username.as_ref(),
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
                String::from(SYSTEM_MESSAGE_NO_TASKS)
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
                command_runtime_state.is_sender_admin(internal_update.sender_username.as_ref());
            let task_summaries = command_runtime_state
                .task_manager()
                .list_recent_tasks(
                    internal_update.chat_identifier,
                    internal_update.sender_username.as_ref(),
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
                    SYSTEM_MESSAGE_NO_TASKS,
                )
                .await;
                return;
            };
            let output_result = command_runtime_state
                .task_manager()
                .get_task_output(
                    last_task_summary.task_identifier,
                    internal_update.chat_identifier,
                    internal_update.sender_username.as_ref(),
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
                command_runtime_state.is_sender_admin(internal_update.sender_username.as_ref());
            let output_result = command_runtime_state
                .task_manager()
                .get_task_output(
                    task_identifier,
                    internal_update.chat_identifier,
                    internal_update.sender_username.as_ref(),
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
                command_runtime_state.is_sender_admin(internal_update.sender_username.as_ref());
            let task_summaries = command_runtime_state
                .task_manager()
                .list_active_tasks(
                    internal_update.chat_identifier,
                    internal_update.sender_username.as_ref(),
                    requester_is_administrator,
                    command_runtime_settings.task_list_maximum_items,
                )
                .await;
            let message_text = if task_summaries.is_empty() {
                String::from(SYSTEM_MESSAGE_NO_ACTIVE_TASKS)
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
                command_runtime_state.is_sender_admin(internal_update.sender_username.as_ref());
            let task_summaries = command_runtime_state
                .task_manager()
                .list_recent_tasks(
                    internal_update.chat_identifier,
                    internal_update.sender_username.as_ref(),
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
                command_runtime_state.is_sender_admin(internal_update.sender_username.as_ref());
            let cancellation_result = command_runtime_state
                .task_manager()
                .request_task_cancellation(
                    task_identifier,
                    internal_update.chat_identifier,
                    internal_update.sender_username.as_ref(),
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
                command_runtime_state.is_sender_admin(internal_update.sender_username.as_ref());
            let retry_lookup = command_runtime_state
                .task_manager()
                .get_retry_task_creation_request(
                    task_identifier,
                    internal_update.chat_identifier,
                    internal_update.sender_username.as_ref(),
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
                    .as_ref()
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

async fn execute_codex_command_and_send_output(
    command_runtime_state: &ServiceState,
    command_runtime_settings: &Arc<ServiceConfiguration>,
    internal_update: &InternalUpdate,
    correlation_identifier: &CorrelationIdentifier,
    incoming_command_name: IncomingCommandName,
    command_line_arguments: Vec<String>,
    usage_message: Option<String>,
) {
    let execution_timeout =
        Duration::from_secs(command_runtime_settings.codex_execution_timeout_seconds);
    let configured_codex_binary_path = command_runtime_settings.codex_binary_path.clone();
    let output_task = spawn_blocking(move || {
        let codex_binary_path = configured_codex_binary_path
            .as_ref()
            .map_or("codex", CodexBinaryPath::as_str);
        let process_output = Command::new(codex_binary_path)
            .args(&command_line_arguments)
            .output();
        process_output.map(|captured_output| {
            let standard_output_text = String::from_utf8_lossy(&captured_output.stdout)
                .trim()
                .to_owned();
            let standard_error_text = String::from_utf8_lossy(&captured_output.stderr)
                .trim()
                .to_owned();
            let output_text = if standard_output_text.is_empty() {
                standard_error_text
            } else if standard_error_text.is_empty() {
                standard_output_text
            } else {
                format!("{standard_output_text}\n{standard_error_text}")
            };
            CodexCommandExecutionResult {
                output_text,
                status_code: captured_output.status.code(),
                succeeded: captured_output.status.success(),
            }
        })
    });
    let execution_result = timeout(execution_timeout, output_task).await;
    let message_text = match execution_result {
        Ok(Ok(Ok(codex_command_execution_result))) => {
            if codex_command_execution_result.succeeded {
                normalize_codex_output(
                    &codex_command_execution_result.output_text,
                    command_runtime_settings.telegram_message_maximum_characters,
                )
            } else {
                let status_code_text = codex_command_execution_result
                    .status_code
                    .map_or_else(|| String::from("unknown"), |status_code| status_code.to_string());
                let output_text = normalize_codex_output(
                    &codex_command_execution_result.output_text,
                    command_runtime_settings.telegram_message_maximum_characters,
                );
                format!("codex command failed: status_code={status_code_text}\n{output_text}")
            }
        }
        Ok(Ok(Err(process_error))) => {
            format!("{ERROR_MESSAGE_CODEX_EXECUTION_PREFIX}: {process_error}")
        }
        Ok(Err(join_error)) => {
            format!("{ERROR_MESSAGE_CODEX_TASK_JOIN_PREFIX}: {join_error}")
        }
        Err(_elapsed) => String::from(SYSTEM_MESSAGE_CODEX_COMMAND_TIMED_OUT),
    };
    let final_message = if let Some(usage_message_text) = usage_message {
        if message_text.is_empty() {
            usage_message_text
        } else {
            message_text
        }
    } else {
        message_text
    };
    send_message_or_log(
        command_runtime_state,
        command_runtime_settings,
        internal_update.chat_identifier,
        internal_update.update_identifier,
        incoming_command_name,
        correlation_identifier,
        &final_message,
    )
    .await;
}

fn parse_command_line_arguments(raw_arguments: &str) -> Result<Vec<String>, String> {
    let mut parsed_arguments = Vec::new();
    let mut current_argument = String::new();
    let mut active_quote_character: Option<char> = None;
    let mut is_escape_active = false;
    for current_character in raw_arguments.chars() {
        if is_escape_active {
            current_argument.push(current_character);
            is_escape_active = false;
            continue;
        }
        if current_character == '\\' {
            is_escape_active = true;
            continue;
        }
        if let Some(active_quote_character_value) = active_quote_character {
            if current_character == active_quote_character_value {
                active_quote_character = None;
                continue;
            }
            current_argument.push(current_character);
            continue;
        }
        if current_character == '"' || current_character == '\'' {
            active_quote_character = Some(current_character);
            continue;
        }
        if current_character.is_whitespace() {
            if !current_argument.is_empty() {
                parsed_arguments.push(current_argument);
                current_argument = String::new();
            }
            continue;
        }
        current_argument.push(current_character);
    }
    if is_escape_active {
        current_argument.push('\\');
    }
    if active_quote_character.is_some() {
        return Err(String::from("unterminated quoted argument"));
    }
    if !current_argument.is_empty() {
        parsed_arguments.push(current_argument);
    }
    if parsed_arguments.is_empty() {
        return Err(String::from("arguments are required"));
    }
    Ok(parsed_arguments)
}

fn spawn_task_execution(
    runtime_state: &ServiceState,
    runtime_settings: &Arc<ServiceConfiguration>,
    chat_identifier: ChatIdentifier,
    update_identifier: UpdateIdentifier,
    correlation_identifier: CorrelationIdentifier,
    task_identifier: TaskIdentifier,
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
                    correlation_identifier = correlation_identifier.as_str(),
                    chat_identifier = chat_identifier.as_i64(),
                    update_identifier = update_identifier.as_i64(),
                    command = "codex",
                    task_identifier = task_identifier.as_u64(),
                    status = "error",
                    error = acquire_error.to_string()
                );
                let _mark_result = task_runtime_state
                    .task_manager()
                    .mark_task_failed(
                        task_identifier,
                        TaskExecutionOutputText::from(format!(
                            "{ERROR_MESSAGE_CODEX_PERMIT_PREFIX}: {acquire_error}"
                        )),
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
                task_runtime_state
                    .metrics()
                    .increment_task_queue_wait_exceeded_total();
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
                .iter()
                .map(|allowed_environment_variable_name| {
                    allowed_environment_variable_name.as_str().to_owned()
                })
                .collect::<Vec<String>>()
                .into(),
            sandbox_auto_cleanup: task_runtime_settings
                .codex_sandbox_auto_cleanup_mode
                .is_enabled(),
            sandbox_enabled: task_runtime_settings.codex_sandbox_enabled,
            sandbox_launcher_arguments: task_runtime_settings
                .codex_sandbox_launcher_arguments
                .iter()
                .map(|sandbox_launcher_argument| sandbox_launcher_argument.as_str().to_owned())
                .collect::<Vec<String>>()
                .into(),
            sandbox_launcher_path: task_runtime_settings
                .codex_sandbox_launcher_path
                .as_ref()
                .map(|sandbox_launcher_path| sandbox_launcher_path.as_str().to_owned().into()),
            sandbox_workspace_root: task_runtime_settings
                .codex_sandbox_workspace_root
                .as_ref()
                .map(|sandbox_workspace_root| sandbox_workspace_root.as_str().to_owned().into()),
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
                    correlation_identifier = correlation_identifier.as_str(),
                    chat_identifier = chat_identifier.as_i64(),
                    update_identifier = update_identifier.as_i64(),
                    command = "codex",
                    task_identifier = task_identifier.as_u64(),
                    status = "error",
                    error = format!("{lookup_error:?}")
                );
                let _mark_result = task_runtime_state
                    .task_manager()
                    .mark_task_failed(
                        task_identifier,
                        TaskExecutionOutputText::from(String::from(
                            ERROR_MESSAGE_TASK_PROMPT_NOT_FOUND,
                        )),
                    )
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
            .map_or_else(|| task_prompt_text.to_string(), str::to_owned);
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
                configured_codex_binary_path
                    .as_ref()
                    .map(CodexBinaryPath::as_str),
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
                    .mark_task_succeeded(
                        task_identifier,
                        TaskExecutionOutputText::from(normalized_output_text.clone()),
                    )
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
                let error_message =
                    format!("{ERROR_MESSAGE_CODEX_EXECUTION_PREFIX}: {execution_error}");
                tracing::error!(
                    event = "codex_execution_error",
                    correlation_identifier = correlation_identifier.as_str(),
                    chat_identifier = chat_identifier.as_i64(),
                    update_identifier = update_identifier.as_i64(),
                    command = "codex",
                    task_identifier = task_identifier.as_u64(),
                    status = "error",
                    error = execution_error.to_string()
                );
                let _mark_result = task_runtime_state
                    .task_manager()
                    .mark_task_failed(
                        task_identifier,
                        TaskExecutionOutputText::from(error_message.clone()),
                    )
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
                let error_message = format!("{ERROR_MESSAGE_CODEX_TASK_JOIN_PREFIX}: {join_error}");
                tracing::error!(
                    event = "codex_task_join_error",
                    correlation_identifier = correlation_identifier.as_str(),
                    chat_identifier = chat_identifier.as_i64(),
                    update_identifier = update_identifier.as_i64(),
                    command = "codex",
                    task_identifier = task_identifier.as_u64(),
                    status = "error",
                    error = join_error.to_string()
                );
                let _mark_result = task_runtime_state
                    .task_manager()
                    .mark_task_failed(
                        task_identifier,
                        TaskExecutionOutputText::from(error_message.clone()),
                    )
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
