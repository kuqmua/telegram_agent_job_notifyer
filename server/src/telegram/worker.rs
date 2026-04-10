use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use codex_cli::exec_prompt_capture_limited_with_binary;
use shared::{
    IncomingCommand, SYSTEM_MESSAGE_CODEX_FINISHED, SYSTEM_MESSAGE_CODEX_STARTED,
    SYSTEM_MESSAGE_CODEX_USAGE, SYSTEM_MESSAGE_HEALTHY, SYSTEM_MESSAGE_UNKNOWN_COMMAND,
    format_system_message, normalize_codex_output, split_text_into_chunks,
};
use tokio::{
    sync::{TryAcquireError, watch},
    task::{JoinSet, spawn_blocking},
    time::{Instant, sleep, timeout},
};

use crate::{
    runtime::ServiceState,
    settings::ServiceConfiguration,
    telegram::{
        api::TelegramApiError,
        commands::{command_name, parse_command},
        model::convert_telegram_update_to_internal,
    },
};

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

                    if !runtime_state.is_chat_authorized(internal_update.chat_identifier) {
                        tracing::warn!(
                            event = "chat_not_authorized",
                            chat_id = internal_update.chat_identifier,
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

                        match parsed_command {
                            IncomingCommand::Health => {
                                if let Err(send_error) = send_system_message(
                                    &command_runtime_state,
                                    &command_runtime_settings,
                                    internal_update.chat_identifier,
                                    internal_update.update_identifier,
                                    parsed_command_name,
                                    &correlation_identifier,
                                    SYSTEM_MESSAGE_HEALTHY,
                                )
                                .await
                                {
                                    log_telegram_send_error(
                                        &command_runtime_state,
                                        &correlation_identifier,
                                        internal_update.chat_identifier,
                                        internal_update.update_identifier,
                                        parsed_command_name,
                                        &send_error,
                                    );
                                }
                            }
                            IncomingCommand::Codex(prompt_text) => {
                                if prompt_text.is_empty() {
                                    if let Err(send_error) = send_system_message(
                                        &command_runtime_state,
                                        &command_runtime_settings,
                                        internal_update.chat_identifier,
                                        internal_update.update_identifier,
                                        "codex",
                                        &correlation_identifier,
                                        SYSTEM_MESSAGE_CODEX_USAGE,
                                    )
                                    .await
                                    {
                                        log_telegram_send_error(
                                            &command_runtime_state,
                                            &correlation_identifier,
                                            internal_update.chat_identifier,
                                            internal_update.update_identifier,
                                            "codex",
                                            &send_error,
                                        );
                                    }
                                    return;
                                }

                                let semaphore_permit =
                                    match command_runtime_state.acquire_codex_permit().await {
                                        Ok(permit) => permit,
                                        Err(acquire_error) => {
                                            command_runtime_state
                                                .metrics()
                                                .increment_codex_execution_error_total();
                                            tracing::error!(
                                                event = "codex_semaphore_error",
                                                correlation_id = correlation_identifier.clone(),
                                                chat_id = internal_update.chat_identifier,
                                                update_id = internal_update.update_identifier,
                                                command = "codex",
                                                status = "error",
                                                error = acquire_error.to_string()
                                            );
                                            return;
                                        }
                                    };

                                if let Err(send_error) = send_system_message(
                                    &command_runtime_state,
                                    &command_runtime_settings,
                                    internal_update.chat_identifier,
                                    internal_update.update_identifier,
                                    "codex",
                                    &correlation_identifier,
                                    SYSTEM_MESSAGE_CODEX_STARTED,
                                )
                                .await
                                {
                                    log_telegram_send_error(
                                        &command_runtime_state,
                                        &correlation_identifier,
                                        internal_update.chat_identifier,
                                        internal_update.update_identifier,
                                        "codex",
                                        &send_error,
                                    );
                                }

                                let execution_started_at = Instant::now();
                                tracing::info!(
                                    event = "codex_execution_start",
                                    correlation_id = correlation_identifier.clone(),
                                    chat_id = internal_update.chat_identifier,
                                    update_id = internal_update.update_identifier,
                                    command = "codex",
                                    status = "started"
                                );

                                let maximum_output_bytes =
                                    command_runtime_settings.codex_output_maximum_bytes;
                                let configured_codex_binary_path =
                                    command_runtime_settings.codex_binary_path.clone();
                                let codex_execution = spawn_blocking(move || {
                                    exec_prompt_capture_limited_with_binary(
                                        &prompt_text,
                                        maximum_output_bytes,
                                        configured_codex_binary_path.as_deref(),
                                    )
                                });

                                let timeout_duration = Duration::from_secs(
                                    command_runtime_settings.codex_execution_timeout_seconds,
                                );
                                let output_text =
                                    match timeout(timeout_duration, codex_execution).await {
                                        Ok(join_result) => match join_result {
                                            Ok(codex_result) => match codex_result {
                                                Ok(codex_output_text) => normalize_codex_output(
                                                    &codex_output_text,
                                                    command_runtime_settings
                                                        .telegram_message_maximum_characters,
                                                ),
                                                Err(execution_error) => {
                                                    command_runtime_state
                                                        .metrics()
                                                        .increment_codex_execution_error_total();
                                                    format!("codex error: {execution_error}")
                                                }
                                            },
                                            Err(join_error) => {
                                                command_runtime_state
                                                    .metrics()
                                                    .increment_codex_execution_error_total();
                                                format!("codex task error: {join_error}")
                                            }
                                        },
                                        Err(_elapsed_error) => {
                                            command_runtime_state
                                                .metrics()
                                                .increment_codex_execution_timeout_total();
                                            command_runtime_state
                                                .metrics()
                                                .increment_codex_execution_error_total();
                                            String::from("codex timed out")
                                        }
                                    };

                                let execution_duration_milliseconds =
                                    execution_started_at.elapsed().as_millis();
                                command_runtime_state
                                    .metrics()
                                    .record_codex_execution_duration_milliseconds(
                                        execution_duration_milliseconds,
                                    );
                                tracing::info!(
                                    event = "codex_execution_finish",
                                    correlation_id = correlation_identifier.clone(),
                                    chat_id = internal_update.chat_identifier,
                                    update_id = internal_update.update_identifier,
                                    command = "codex",
                                    duration_ms = execution_duration_milliseconds,
                                    status = "finished"
                                );

                                drop(semaphore_permit);

                                let final_message_text =
                                    format!("{SYSTEM_MESSAGE_CODEX_FINISHED}\n{output_text}");
                                if let Err(send_error) = send_system_message(
                                    &command_runtime_state,
                                    &command_runtime_settings,
                                    internal_update.chat_identifier,
                                    internal_update.update_identifier,
                                    "codex",
                                    &correlation_identifier,
                                    &final_message_text,
                                )
                                .await
                                {
                                    log_telegram_send_error(
                                        &command_runtime_state,
                                        &correlation_identifier,
                                        internal_update.chat_identifier,
                                        internal_update.update_identifier,
                                        "codex",
                                        &send_error,
                                    );
                                }
                            }
                            IncomingCommand::Unknown => {
                                if let Err(send_error) = send_system_message(
                                    &command_runtime_state,
                                    &command_runtime_settings,
                                    internal_update.chat_identifier,
                                    internal_update.update_identifier,
                                    parsed_command_name,
                                    &correlation_identifier,
                                    SYSTEM_MESSAGE_UNKNOWN_COMMAND,
                                )
                                .await
                                {
                                    log_telegram_send_error(
                                        &command_runtime_state,
                                        &correlation_identifier,
                                        internal_update.chat_identifier,
                                        internal_update.update_identifier,
                                        parsed_command_name,
                                        &send_error,
                                    );
                                }
                            }
                        }
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
