use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::{sync::watch, time::sleep};

use crate::{
    runtime::ServiceState,
    settings::ServiceConfiguration,
    shared::{
        SYSTEM_MESSAGE_HEALTHY, SYSTEM_MESSAGE_UNKNOWN_COMMAND, UpdateIdentifier,
        format_system_message, split_text_into_chunks,
    },
    telegram::model::convert_telegram_update_to_internal,
};

const SYSTEM_MESSAGE_HELP_TELEGRAM_ONLY: &str = "Commands:\n/health - bot health\n/help - this \
                                                 help\n/whoami - sender identity\n/version - \
                                                 build info";
const TELEGRAM_MESSAGE_MAXIMUM_CHARACTERS: usize = 3_500;

#[derive(Debug)]
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

#[derive(Debug)]
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
    let mut update_offset = runtime_settings.polling_initial_offset;
    let mut processed_update_cache = ProcessedUpdateCache {
        insertion_order: VecDeque::new(),
        known_identifiers: HashSet::new(),
        maximum_size: 1_024,
    };
    let mut polling_backoff = PollingBackoff {
        current_delay_milliseconds: runtime_settings.polling_backoff_minimum_milliseconds,
        maximum_delay_milliseconds: runtime_settings.polling_backoff_maximum_milliseconds,
        minimum_delay_milliseconds: runtime_settings.polling_backoff_minimum_milliseconds,
    };
    while !*shutdown_receiver.borrow() {
        let polling_result = runtime_state
            .telegram_client()
            .get_updates(update_offset, runtime_settings.telegram_poll_timeout_seconds)
            .await;
        match polling_result {
            Ok(telegram_updates) => {
                runtime_state.set_polling_ready(true);
                polling_backoff.reset();
                for telegram_update in telegram_updates {
                    if telegram_update.update_identifier.as_i64() >= update_offset {
                        update_offset =
                            telegram_update.update_identifier.as_i64().saturating_add(1);
                    }
                    if processed_update_cache.contains(telegram_update.update_identifier) {
                        continue;
                    }
                    processed_update_cache.insert(telegram_update.update_identifier);
                    let Some(internal_update) =
                        convert_telegram_update_to_internal(telegram_update)
                    else {
                        continue;
                    };
                    if !runtime_state.is_update_authorized(
                        internal_update.chat_identifier,
                        internal_update.sender_username.as_ref(),
                    ) {
                        tracing::warn!(
                            event = "update_not_authorized",
                            chat_identifier = internal_update.chat_identifier.as_i64(),
                            update_identifier = internal_update.update_identifier.as_i64(),
                            status = "ignored"
                        );
                        continue;
                    }
                    let incoming_message_text = internal_update.message_text.as_str().trim();
                    let response_text = if incoming_message_text.eq_ignore_ascii_case("/health") {
                        String::from(SYSTEM_MESSAGE_HEALTHY)
                    } else if incoming_message_text.eq_ignore_ascii_case("/help") {
                        String::from(SYSTEM_MESSAGE_HELP_TELEGRAM_ONLY)
                    } else if incoming_message_text.eq_ignore_ascii_case("/version") {
                        let git_hash = option_env!("SERVER_GIT_HASH").unwrap_or("unknown");
                        let build_time_utc =
                            option_env!("SERVER_BUILD_TIME_UTC").unwrap_or("unknown");
                        format!(
                            "server_version:\ngit_hash={git_hash}\nbuild_time_utc={build_time_utc}"
                        )
                    } else if incoming_message_text.eq_ignore_ascii_case("/whoami") {
                        let username_text = internal_update
                            .sender_username
                            .as_ref()
                            .map_or("<missing>", |sender_username| sender_username.as_str());
                        format!(
                            "whoami:\nchat_id={}\nusername={username_text}",
                            internal_update.chat_identifier.as_i64()
                        )
                    } else {
                        String::from(SYSTEM_MESSAGE_UNKNOWN_COMMAND)
                    };
                    let formatted_message_text = format_system_message(&response_text);
                    let message_chunks = split_text_into_chunks(
                        &formatted_message_text,
                        TELEGRAM_MESSAGE_MAXIMUM_CHARACTERS,
                    );
                    for message_chunk in message_chunks {
                        let send_result = runtime_state
                            .telegram_client()
                            .send_message(internal_update.chat_identifier, &message_chunk)
                            .await;
                        if let Err(send_error) = send_result {
                            tracing::error!(
                                event = "telegram_send_error",
                                chat_identifier = internal_update.chat_identifier.as_i64(),
                                update_identifier = internal_update.update_identifier.as_i64(),
                                status = "error",
                                error = send_error.to_string()
                            );
                            break;
                        }
                    }
                }
            }
            Err(polling_error) => {
                let delay_duration = polling_backoff.take_delay();
                if !polling_error.is_temporary() {
                    runtime_state.set_polling_ready(false);
                }
                tracing::warn!(
                    event = "polling_error",
                    status = "retrying",
                    delay_ms = delay_duration.as_millis(),
                    error = polling_error.to_string()
                );
                sleep(delay_duration).await;
            }
        }
    }
    tracing::info!(event = "polling_stop", status = "shutdown_signal");
}
