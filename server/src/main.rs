pub mod failures {
    use std::io::Error as InputOutputError;

    use thiserror::Error;

    use crate::{
        settings::EnvironmentError,
        telegram::application_programming_interface::TelegramApplicationProgrammingInterfaceError,
    };

    #[derive(Debug, Error)]
    pub enum ServiceFailure {
        #[error("configuration error: {0}")]
        Configuration(#[from] EnvironmentError),
        #[error("failed to build http client: {0}")]
        HyperTextTransferProtocolClientBuild(#[from] reqwest::Error),
        #[error("io error: {0}")]
        InputOutput(#[from] InputOutputError),
        #[error("telegram api error: {0}")]
        TelegramApplicationProgrammingInterface(
            #[from] TelegramApplicationProgrammingInterfaceError,
        ),
    }
}

pub mod settings {
    use std::env;

    use thiserror::Error;

    use crate::shared::{ChatIdentifier, SenderUsername};

    const ENVIRONMENT_NAME_TELEGRAM_APPLICATION_PROGRAMMING_INTERFACE_BASE_UNIFORM_RESOURCE_LOCATOR:
        &str = "TELEGRAM_API_BASE_URL";
    const ENVIRONMENT_NAME_TELEGRAM_ALLOWED_USERNAME: &str = "TELEGRAM_ALLOWED_USERNAME";
    const ENVIRONMENT_NAME_TELEGRAM_BOT_TOKEN: &str = "TELEGRAM_BOT_TOKEN";
    const ENVIRONMENT_NAME_TELEGRAM_CHAT_IDENTIFIER: &str = "TELEGRAM_CHAT_ID";
    const ENVIRONMENT_NAME_TELEGRAM_HTTP_TIMEOUT_SECONDS: &str = "TELEGRAM_HTTP_TIMEOUT_SECONDS";
    const ENVIRONMENT_NAME_TELEGRAM_POLL_BACKOFF_MAXIMUM_MILLISECONDS: &str =
        "TELEGRAM_POLL_BACKOFF_MAX_MS";
    const ENVIRONMENT_NAME_TELEGRAM_POLL_BACKOFF_MINIMUM_MILLISECONDS: &str =
        "TELEGRAM_POLL_BACKOFF_MIN_MS";
    const ENVIRONMENT_NAME_TELEGRAM_POLL_INITIAL_OFFSET: &str = "TELEGRAM_POLL_INITIAL_OFFSET";
    const ENVIRONMENT_NAME_TELEGRAM_POLL_TIMEOUT_SECONDS: &str = "TELEGRAM_POLL_TIMEOUT_SECONDS";

    const DEFAULT_TELEGRAM_APPLICATION_PROGRAMMING_INTERFACE_BASE_UNIFORM_RESOURCE_LOCATOR: &str =
        "https://api.telegram.org";
    const DEFAULT_TELEGRAM_HTTP_TIMEOUT_SECONDS: u64 = 35;
    const DEFAULT_TELEGRAM_POLL_BACKOFF_MAXIMUM_MILLISECONDS: u64 = 5_000;
    const DEFAULT_TELEGRAM_POLL_BACKOFF_MINIMUM_MILLISECONDS: u64 = 500;
    const DEFAULT_TELEGRAM_POLL_INITIAL_OFFSET: i64 = 0;
    const DEFAULT_TELEGRAM_POLL_TIMEOUT_SECONDS: u64 = 30;

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct TelegramApplicationProgrammingInterfaceBaseUniformResourceLocator(String);

    impl TelegramApplicationProgrammingInterfaceBaseUniformResourceLocator {
        #[must_use]
        pub fn as_str(&self) -> &str {
            &self.0
        }
    }

    impl From<String> for TelegramApplicationProgrammingInterfaceBaseUniformResourceLocator {
        fn from(value: String) -> Self {
            Self(value)
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct TelegramBotToken(String);

    impl TelegramBotToken {
        #[must_use]
        pub fn as_str(&self) -> &str {
            &self.0
        }
    }

    impl From<String> for TelegramBotToken {
        fn from(value: String) -> Self {
            Self(value)
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct ServiceConfiguration {
        pub polling_backoff_maximum_milliseconds: u64,
        pub polling_backoff_minimum_milliseconds: u64,
        pub polling_initial_offset: i64,
        pub telegram_allowed_username: Option<SenderUsername>,
        pub telegram_application_programming_interface_base_uniform_resource_locator:
            TelegramApplicationProgrammingInterfaceBaseUniformResourceLocator,
        pub telegram_bot_token: TelegramBotToken,
        pub telegram_chat_identifier: Option<ChatIdentifier>,
        pub telegram_hyper_text_transfer_protocol_timeout_seconds: u64,
        pub telegram_poll_timeout_seconds: u64,
    }

    #[derive(Debug, Error)]
    pub enum EnvironmentError {
        #[error("invalid environment variable {variable_name}: {message}")]
        Invalid {
            message: String,
            variable_name: &'static str,
        },
        #[error("missing required environment variable {variable_name}")]
        Missing { variable_name: &'static str },
    }

    impl ServiceConfiguration {
        pub fn from_env() -> Result<Self, EnvironmentError> {
            let telegram_bot_token_raw = env::var(ENVIRONMENT_NAME_TELEGRAM_BOT_TOKEN).map_err(
                |environment_variable_error| match environment_variable_error {
                    env::VarError::NotPresent => EnvironmentError::Missing {
                        variable_name: ENVIRONMENT_NAME_TELEGRAM_BOT_TOKEN,
                    },
                    env::VarError::NotUnicode(_) => EnvironmentError::Invalid {
                        message: String::from("value must be valid UTF-8"),
                        variable_name: ENVIRONMENT_NAME_TELEGRAM_BOT_TOKEN,
                    },
                },
            )?;
            let telegram_bot_token = parse_non_empty_string(
                ENVIRONMENT_NAME_TELEGRAM_BOT_TOKEN,
                &telegram_bot_token_raw,
            )?;
            let telegram_application_programming_interface_base_uniform_resource_locator =
                read_optional_environment_variable(
                    ENVIRONMENT_NAME_TELEGRAM_APPLICATION_PROGRAMMING_INTERFACE_BASE_UNIFORM_RESOURCE_LOCATOR,
                    parse_non_empty_string,
                )?
                .unwrap_or_else(|| {
                    String::from(
                        DEFAULT_TELEGRAM_APPLICATION_PROGRAMMING_INTERFACE_BASE_UNIFORM_RESOURCE_LOCATOR,
                    )
                });
            let telegram_chat_identifier = read_optional_environment_variable(
                ENVIRONMENT_NAME_TELEGRAM_CHAT_IDENTIFIER,
                parse_i64,
            )?
            .map(ChatIdentifier::from);
            let telegram_allowed_username = read_optional_environment_variable(
                ENVIRONMENT_NAME_TELEGRAM_ALLOWED_USERNAME,
                |variable_name, variable_value| {
                    let normalized_username = variable_value
                        .trim()
                        .trim_start_matches('@')
                        .to_ascii_lowercase();
                    if normalized_username.is_empty() {
                        return Err(EnvironmentError::Invalid {
                            message: String::from("value must not be empty"),
                            variable_name,
                        });
                    }
                    if normalized_username.chars().any(char::is_whitespace) {
                        return Err(EnvironmentError::Invalid {
                            message: String::from("value must not contain whitespace"),
                            variable_name,
                        });
                    }
                    Ok(SenderUsername::from(normalized_username))
                },
            )?;
            let telegram_poll_timeout_seconds = read_optional_environment_variable(
                ENVIRONMENT_NAME_TELEGRAM_POLL_TIMEOUT_SECONDS,
                parse_positive_u64,
            )?
            .unwrap_or(DEFAULT_TELEGRAM_POLL_TIMEOUT_SECONDS);
            let telegram_hyper_text_transfer_protocol_timeout_seconds =
                read_optional_environment_variable(
                    ENVIRONMENT_NAME_TELEGRAM_HTTP_TIMEOUT_SECONDS,
                    parse_positive_u64,
                )?
                .unwrap_or(DEFAULT_TELEGRAM_HTTP_TIMEOUT_SECONDS);
            if telegram_hyper_text_transfer_protocol_timeout_seconds
                <= telegram_poll_timeout_seconds
            {
                return Err(EnvironmentError::Invalid {
                    message: String::from("must be greater than TELEGRAM_POLL_TIMEOUT_SECONDS"),
                    variable_name: ENVIRONMENT_NAME_TELEGRAM_HTTP_TIMEOUT_SECONDS,
                });
            }
            let polling_initial_offset = read_optional_environment_variable(
                ENVIRONMENT_NAME_TELEGRAM_POLL_INITIAL_OFFSET,
                parse_i64,
            )?
            .unwrap_or(DEFAULT_TELEGRAM_POLL_INITIAL_OFFSET);
            let polling_backoff_minimum_milliseconds = read_optional_environment_variable(
                ENVIRONMENT_NAME_TELEGRAM_POLL_BACKOFF_MINIMUM_MILLISECONDS,
                parse_positive_u64,
            )?
            .unwrap_or(DEFAULT_TELEGRAM_POLL_BACKOFF_MINIMUM_MILLISECONDS);
            let polling_backoff_maximum_milliseconds = read_optional_environment_variable(
                ENVIRONMENT_NAME_TELEGRAM_POLL_BACKOFF_MAXIMUM_MILLISECONDS,
                parse_positive_u64,
            )?
            .unwrap_or(DEFAULT_TELEGRAM_POLL_BACKOFF_MAXIMUM_MILLISECONDS);
            if polling_backoff_maximum_milliseconds < polling_backoff_minimum_milliseconds {
                return Err(EnvironmentError::Invalid {
                    message: String::from(
                        "must be greater than or equal to TELEGRAM_POLL_BACKOFF_MIN_MS",
                    ),
                    variable_name: ENVIRONMENT_NAME_TELEGRAM_POLL_BACKOFF_MAXIMUM_MILLISECONDS,
                });
            }
            Ok(Self {
                polling_backoff_maximum_milliseconds,
                polling_backoff_minimum_milliseconds,
                polling_initial_offset,
                telegram_allowed_username,
                telegram_application_programming_interface_base_uniform_resource_locator:
                    telegram_application_programming_interface_base_uniform_resource_locator.into(),
                telegram_bot_token: telegram_bot_token.into(),
                telegram_chat_identifier,
                telegram_hyper_text_transfer_protocol_timeout_seconds,
                telegram_poll_timeout_seconds,
            })
        }
    }

    fn parse_i64(
        variable_name: &'static str,
        variable_value: &str,
    ) -> Result<i64, EnvironmentError> {
        variable_value
            .trim()
            .parse::<i64>()
            .map_err(|parse_error| EnvironmentError::Invalid {
                message: format!("must be a valid i64: {parse_error}"),
                variable_name,
            })
    }

    fn parse_non_empty_string(
        variable_name: &'static str,
        variable_value: &str,
    ) -> Result<String, EnvironmentError> {
        let trimmed_value = variable_value.trim();
        if trimmed_value.is_empty() {
            return Err(EnvironmentError::Invalid {
                message: String::from("value must not be empty"),
                variable_name,
            });
        }
        Ok(String::from(trimmed_value))
    }

    fn parse_positive_u64(
        variable_name: &'static str,
        variable_value: &str,
    ) -> Result<u64, EnvironmentError> {
        let parsed_value = variable_value
            .trim()
            .parse::<u64>()
            .map_err(|parse_error| EnvironmentError::Invalid {
                message: format!("must be a valid u64: {parse_error}"),
                variable_name,
            })?;
        if parsed_value == 0 {
            return Err(EnvironmentError::Invalid {
                message: String::from("must be greater than zero"),
                variable_name,
            });
        }
        Ok(parsed_value)
    }

    fn read_optional_environment_variable<T>(
        variable_name: &'static str,
        parser: fn(&'static str, &str) -> Result<T, EnvironmentError>,
    ) -> Result<Option<T>, EnvironmentError> {
        match env::var(variable_name) {
            Ok(variable_value) => parser(variable_name, &variable_value).map(Some),
            Err(env::VarError::NotPresent) => Ok(None),
            Err(env::VarError::NotUnicode(_)) => Err(EnvironmentError::Invalid {
                message: String::from("value must be valid UTF-8"),
                variable_name,
            }),
        }
    }
}

pub mod telegram {
    pub mod model {
        use std::{fmt, ops::Deref, slice::Iter};

        use serde::{Deserialize, Serialize};

        use crate::shared::{
            ChatIdentifier, SenderUsername, TelegramMessageText, UpdateIdentifier,
        };

        #[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
        #[serde(transparent)]
        pub struct TelegramApplicationProgrammingInterfaceDescription(String);

        impl TelegramApplicationProgrammingInterfaceDescription {
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            #[must_use]
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl From<String> for TelegramApplicationProgrammingInterfaceDescription {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl Deref for TelegramApplicationProgrammingInterfaceDescription {
            type Target = str;

            fn deref(&self) -> &Self::Target {
                self.as_str()
            }
        }

        impl fmt::Display for TelegramApplicationProgrammingInterfaceDescription {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        #[derive(Debug, Clone, Deserialize)]
        #[serde(transparent)]
        pub struct TelegramUpdates(Vec<TelegramUpdate>);

        impl TelegramUpdates {
            #[must_use]
            pub fn into_inner(self) -> Vec<TelegramUpdate> {
                self.0
            }

            pub fn iter(&self) -> Iter<'_, TelegramUpdate> {
                self.0.iter()
            }
        }

        impl<'telegram_updates> IntoIterator for &'telegram_updates TelegramUpdates {
            type IntoIter = Iter<'telegram_updates, TelegramUpdate>;
            type Item = &'telegram_updates TelegramUpdate;

            fn into_iter(self) -> Self::IntoIter {
                self.iter()
            }
        }

        #[derive(Debug, Clone, Deserialize)]
        pub struct TelegramGetUpdatesResponse {
            pub description: Option<TelegramApplicationProgrammingInterfaceDescription>,
            pub ok: bool,
            pub result: TelegramUpdates,
        }
        #[derive(Debug, Clone, Deserialize)]
        pub struct TelegramUpdate {
            pub message: Option<TelegramIncomingMessage>,
            #[serde(rename = "update_id")]
            pub update_identifier: UpdateIdentifier,
        }
        #[derive(Debug, Clone, Deserialize)]
        pub struct TelegramIncomingMessage {
            pub chat: TelegramChat,
            pub from: Option<TelegramUser>,
            pub text: Option<TelegramMessageText>,
        }
        #[derive(Debug, Clone, Copy, Deserialize)]
        pub struct TelegramChat {
            #[serde(rename = "id")]
            pub chat_identifier: ChatIdentifier,
        }
        #[derive(Debug, Clone, Deserialize)]
        pub struct TelegramUser {
            pub username: Option<SenderUsername>,
        }
        #[derive(Debug, Clone, Serialize)]
        pub struct TelegramSendMessageRequest {
            #[serde(rename = "chat_id")]
            pub chat_identifier: ChatIdentifier,
            pub text: TelegramMessageText,
        }
        #[derive(Debug, Clone, Deserialize)]
        pub struct TelegramSendMessageResponse {
            pub description: Option<TelegramApplicationProgrammingInterfaceDescription>,
            pub ok: bool,
        }
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct InternalUpdate {
            pub chat_identifier: ChatIdentifier,
            pub message_text: TelegramMessageText,
            pub sender_username: Option<SenderUsername>,
            pub update_identifier: UpdateIdentifier,
        }
        #[must_use]
        pub fn convert_telegram_update_to_internal(
            telegram_update: TelegramUpdate,
        ) -> Option<InternalUpdate> {
            let incoming_message = telegram_update.message?;
            let message_text = incoming_message.text?;
            Some(InternalUpdate {
                chat_identifier: incoming_message.chat.chat_identifier,
                message_text,
                sender_username: incoming_message.from.and_then(|sender| sender.username),
                update_identifier: telegram_update.update_identifier,
            })
        }
    }

    pub mod application_programming_interface {
        use std::time::Duration;

        use reqwest::{Client, StatusCode};
        use thiserror::Error;

        use crate::{
            settings::{
                TelegramApplicationProgrammingInterfaceBaseUniformResourceLocator, TelegramBotToken,
            },
            shared::{ChatIdentifier, TelegramMessageText},
            telegram::model::{
                TelegramApplicationProgrammingInterfaceDescription, TelegramGetUpdatesResponse,
                TelegramSendMessageRequest, TelegramSendMessageResponse, TelegramUpdate,
            },
        };
        #[derive(Clone, Debug)]
        pub struct TelegramApplicationProgrammingInterfaceClient {
            application_programming_interface_base_uniform_resource_locator:
                TelegramApplicationProgrammingInterfaceBaseUniformResourceLocator,
            bot_token: TelegramBotToken,
            hyper_text_transfer_protocol_client: Client,
        }
        #[derive(Debug, Error)]
        pub enum TelegramApplicationProgrammingInterfaceError {
            #[error("telegram api returned error: {0}")]
            ApplicationProgrammingInterfaceReported(String),
            #[error("telegram http status {status_code}: {response_body}")]
            HyperTextTransferProtocolStatus {
                response_body: String,
                status_code: StatusCode,
            },
            #[error("request failed: {0}")]
            Request(#[from] reqwest::Error),
        }
        impl TelegramApplicationProgrammingInterfaceError {
            #[must_use]
            pub fn is_temporary(&self) -> bool {
                match self {
                    Self::ApplicationProgrammingInterfaceReported(_) => false,
                    Self::HyperTextTransferProtocolStatus { status_code, .. } => {
                        *status_code == StatusCode::TOO_MANY_REQUESTS
                            || status_code.is_server_error()
                    }
                    Self::Request(request_error) => {
                        request_error.is_connect()
                            || request_error.is_request()
                            || request_error.is_timeout()
                    }
                }
            }
        }
        impl TelegramApplicationProgrammingInterfaceClient {
            pub async fn get_updates(
                &self,
                update_offset: i64,
                timeout_seconds: u64,
            ) -> Result<Vec<TelegramUpdate>, TelegramApplicationProgrammingInterfaceError>
            {
                let request_uniform_resource_locator = format!(
                    "{}/bot{}/getUpdates",
                    self.application_programming_interface_base_uniform_resource_locator
                        .as_str(),
                    self.bot_token.as_str(),
                );
                let response = self
                    .hyper_text_transfer_protocol_client
                    .get(request_uniform_resource_locator)
                    .query(&[
                        ("offset", update_offset.to_string()),
                        ("timeout", timeout_seconds.to_string()),
                    ])
                    .send()
                    .await?;
                if !response.status().is_success() {
                    let status_code = response.status();
                    let response_body = response
                        .text()
                        .await
                        .unwrap_or_else(|_| String::from("<unreadable response body>"));
                    return Err(
                        TelegramApplicationProgrammingInterfaceError::HyperTextTransferProtocolStatus {
                            response_body,
                            status_code,
                        },
                    );
                }
                let updates_response = response.json::<TelegramGetUpdatesResponse>().await?;
                if !updates_response.ok {
                    return Err(TelegramApplicationProgrammingInterfaceError::ApplicationProgrammingInterfaceReported(
                        updates_response
                            .description
                            .map_or_else(
                                || String::from("getUpdates returned ok=false"),
                                TelegramApplicationProgrammingInterfaceDescription::into_inner,
                            ),
                    ));
                }
                Ok(updates_response.result.into_inner())
            }

            pub fn new(
                application_programming_interface_base_uniform_resource_locator:
                    TelegramApplicationProgrammingInterfaceBaseUniformResourceLocator,
                bot_token: TelegramBotToken,
                request_timeout_seconds: u64,
            ) -> Result<Self, reqwest::Error> {
                let request_timeout = Duration::from_secs(request_timeout_seconds);
                let hyper_text_transfer_protocol_client =
                    Client::builder().timeout(request_timeout).build()?;
                Ok(Self {
                    application_programming_interface_base_uniform_resource_locator,
                    bot_token,
                    hyper_text_transfer_protocol_client,
                })
            }

            pub async fn send_message(
                &self,
                chat_identifier: ChatIdentifier,
                text: &str,
            ) -> Result<(), TelegramApplicationProgrammingInterfaceError> {
                let request_uniform_resource_locator = format!(
                    "{}/bot{}/sendMessage",
                    self.application_programming_interface_base_uniform_resource_locator
                        .as_str(),
                    self.bot_token.as_str(),
                );
                let response = self
                    .hyper_text_transfer_protocol_client
                    .post(request_uniform_resource_locator)
                    .json(&TelegramSendMessageRequest {
                        chat_identifier,
                        text: TelegramMessageText::from(text.to_owned()),
                    })
                    .send()
                    .await?;
                if !response.status().is_success() {
                    let status_code = response.status();
                    let response_body = response
                        .text()
                        .await
                        .unwrap_or_else(|_| String::from("<unreadable response body>"));
                    return Err(
                        TelegramApplicationProgrammingInterfaceError::HyperTextTransferProtocolStatus {
                            response_body,
                            status_code,
                        },
                    );
                }
                let send_message_response = response.json::<TelegramSendMessageResponse>().await?;
                if !send_message_response.ok {
                    return Err(TelegramApplicationProgrammingInterfaceError::ApplicationProgrammingInterfaceReported(
                        send_message_response
                            .description
                            .map_or_else(
                                || String::from("sendMessage returned ok=false"),
                                TelegramApplicationProgrammingInterfaceDescription::into_inner,
                            ),
                    ));
                }
                Ok(())
            }
        }
    }

    pub mod worker {
        use std::{
            collections::{HashSet, VecDeque},
            path::PathBuf,
            sync::Arc,
            time::{Duration, SystemTime, UNIX_EPOCH},
        };

        use codex_task_runner_shared::{
            DEFAULT_MANAGED_DIRECTORY_NAME, TaskRunnerConfiguration,
            resolve_codex_binary_from_environment, resolve_log_maximum_bytes_from_environment,
            run_tasks_json,
        };
        use tokio::{
            sync::watch,
            task::spawn_blocking,
            time::{Instant, sleep},
        };

        use crate::{
            runtime::ServiceState,
            settings::ServiceConfiguration,
            shared::{
                SYSTEM_MESSAGE_HEALTHY, SYSTEM_MESSAGE_UNKNOWN_COMMAND, UpdateIdentifier,
                format_system_message, split_text_into_chunks,
            },
            telegram::model::convert_telegram_update_to_internal,
        };

        const SYSTEM_MESSAGE_HELP_TELEGRAM_ONLY: &str =
            "Commands:\n/health - bot health\n/help - this help\n/run_tasks <json_array> - run \
             cdx tasks\n/whoami - sender identity\n/version - build info";
        const SYSTEM_MESSAGE_RUN_TASKS_USAGE: &str =
            "Usage: /run_tasks [{\"prompt\":\"...\",\"repeat\":1}]";
        const SYSTEM_MESSAGE_RUN_TASKS_STARTED: &str = "run_tasks: started";
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
                    let timestamp_value =
                        u64::try_from(nanoseconds_since_epoch).unwrap_or(u64::MAX);
                    let bitmask = jitter_window.next_power_of_two().saturating_sub(1);
                    let candidate_value = timestamp_value & bitmask;
                    candidate_value.min(jitter_window)
                };
                let delay_with_jitter =
                    self.current_delay_milliseconds.saturating_add(jitter_value);
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
                            let incoming_message_text =
                                internal_update.message_text.as_str().trim();
                            let run_tasks_payload = incoming_message_text
                                .strip_prefix("/run_tasks")
                                .map(str::trim)
                                .filter(|payload| !payload.is_empty());
                            if let Some(tasks_json_payload) = run_tasks_payload {
                                let started_message =
                                    format_system_message(SYSTEM_MESSAGE_RUN_TASKS_STARTED);
                                let started_message_chunks = split_text_into_chunks(
                                    &started_message,
                                    TELEGRAM_MESSAGE_MAXIMUM_CHARACTERS,
                                );
                                for message_chunk in started_message_chunks {
                                    let send_result = runtime_state
                                        .telegram_client()
                                        .send_message(
                                            internal_update.chat_identifier,
                                            &message_chunk,
                                        )
                                        .await;
                                    if let Err(send_error) = send_result {
                                        tracing::error!(
                                            event = "telegram_send_error",
                                            chat_identifier =
                                                internal_update.chat_identifier.as_i64(),
                                            update_identifier =
                                                internal_update.update_identifier.as_i64(),
                                            status = "error",
                                            error = send_error.to_string()
                                        );
                                        break;
                                    }
                                }
                                let tasks_json_owned = tasks_json_payload.to_owned();
                                let task_run_result = spawn_blocking(move || {
                                    let log_maximum_bytes =
                                        resolve_log_maximum_bytes_from_environment()?;
                                    let task_runner_configuration = TaskRunnerConfiguration {
                                        codex_binary_path: resolve_codex_binary_from_environment(),
                                        log_maximum_bytes,
                                        managed_directory_path: PathBuf::from(
                                            DEFAULT_MANAGED_DIRECTORY_NAME,
                                        ),
                                    };
                                    run_tasks_json(
                                        tasks_json_owned.as_str(),
                                        &task_runner_configuration,
                                    )
                                })
                                .await;
                                let response_text = match task_run_result {
                                    Ok(Ok(())) => String::from("run_tasks: completed"),
                                    Ok(Err(error)) => format!("run_tasks: failed\n{error}"),
                                    Err(join_error) => {
                                        format!("run_tasks: failed to join worker: {join_error}")
                                    }
                                };
                                let formatted_message_text = format_system_message(&response_text);
                                let message_chunks = split_text_into_chunks(
                                    &formatted_message_text,
                                    TELEGRAM_MESSAGE_MAXIMUM_CHARACTERS,
                                );
                                for message_chunk in message_chunks {
                                    let send_result = runtime_state
                                        .telegram_client()
                                        .send_message(
                                            internal_update.chat_identifier,
                                            &message_chunk,
                                        )
                                        .await;
                                    if let Err(send_error) = send_result {
                                        tracing::error!(
                                            event = "telegram_send_error",
                                            chat_identifier =
                                                internal_update.chat_identifier.as_i64(),
                                            update_identifier =
                                                internal_update.update_identifier.as_i64(),
                                            status = "error",
                                            error = send_error.to_string()
                                        );
                                        break;
                                    }
                                }
                                continue;
                            }
                            let response_text = if incoming_message_text
                                .eq_ignore_ascii_case("/health")
                            {
                                String::from(SYSTEM_MESSAGE_HEALTHY)
                            } else if incoming_message_text.eq_ignore_ascii_case("/help") {
                                String::from(SYSTEM_MESSAGE_HELP_TELEGRAM_ONLY)
                            } else if incoming_message_text.eq_ignore_ascii_case("/run_tasks") {
                                String::from(SYSTEM_MESSAGE_RUN_TASKS_USAGE)
                            } else if incoming_message_text.eq_ignore_ascii_case("/version") {
                                let git_hash = option_env!("SERVER_GIT_HASH").unwrap_or("unknown");
                                let build_time_utc =
                                    option_env!("SERVER_BUILD_TIME_UTC").unwrap_or("unknown");
                                format!(
                                    "server_version:\ngit_hash={git_hash}\\
                                     nbuild_time_utc={build_time_utc}"
                                )
                            } else if incoming_message_text.eq_ignore_ascii_case("/whoami") {
                                let username_text = internal_update
                                    .sender_username
                                    .as_ref()
                                    .map_or("<missing>", |sender_username| {
                                        sender_username.as_str()
                                    });
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
                                        update_identifier =
                                            internal_update.update_identifier.as_i64(),
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
                        let Some(backoff_deadline) = Instant::now().checked_add(delay_duration)
                        else {
                            break;
                        };
                        while Instant::now() < backoff_deadline {
                            if *shutdown_receiver.borrow() {
                                break;
                            }
                            let remaining_duration =
                                backoff_deadline.saturating_duration_since(Instant::now());
                            let sleep_step_duration =
                                remaining_duration.min(Duration::from_millis(250));
                            sleep(sleep_step_duration).await;
                        }
                        if *shutdown_receiver.borrow() {
                            break;
                        }
                    }
                }
            }
            tracing::info!(event = "polling_stop", status = "shutdown_signal");
        }
    }
}

pub mod runtime {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use crate::{
        settings::ServiceConfiguration,
        shared::{ChatIdentifier, SenderUsername},
        telegram::application_programming_interface::TelegramApplicationProgrammingInterfaceClient,
    };

    #[derive(Clone, Debug)]
    pub struct ServiceState {
        polling_ready: Arc<AtomicBool>,
        telegram_allowed_username: Option<SenderUsername>,
        telegram_chat_identifier: Option<ChatIdentifier>,
        telegram_client: TelegramApplicationProgrammingInterfaceClient,
    }

    impl ServiceState {
        #[must_use]
        pub fn is_polling_ready(&self) -> bool {
            self.polling_ready.load(Ordering::Relaxed)
        }

        #[must_use]
        pub fn is_update_authorized(
            &self,
            incoming_chat_identifier: ChatIdentifier,
            incoming_sender_username: Option<&SenderUsername>,
        ) -> bool {
            if self
                .telegram_chat_identifier
                .is_some_and(|configured_chat_identifier| {
                    configured_chat_identifier != incoming_chat_identifier
                })
            {
                return false;
            }
            if let Some(configured_allowed_username) = &self.telegram_allowed_username {
                let Some(incoming_username) = incoming_sender_username else {
                    return false;
                };
                return configured_allowed_username
                    .as_str()
                    .eq_ignore_ascii_case(incoming_username.as_str());
            }
            true
        }

        #[must_use]
        pub fn new(
            telegram_client: TelegramApplicationProgrammingInterfaceClient,
            service_configuration: &ServiceConfiguration,
        ) -> Self {
            Self {
                polling_ready: Arc::new(AtomicBool::new(false)),
                telegram_allowed_username: service_configuration.telegram_allowed_username.clone(),
                telegram_chat_identifier: service_configuration.telegram_chat_identifier,
                telegram_client,
            }
        }

        pub fn set_polling_ready(&self, value: bool) {
            self.polling_ready.store(value, Ordering::Relaxed);
        }

        #[must_use]
        pub const fn telegram_client(&self) -> &TelegramApplicationProgrammingInterfaceClient {
            &self.telegram_client
        }
    }
}

use std::{io::Error as InputOutputError, sync::Arc, time::Duration as StandardDuration};

use axum as _;
use dotenvy as _;
use serde_json as _;
pub use telegram_agent_shared as shared;
use tokio::{signal::ctrl_c, sync::watch, time::timeout};
use tracing_subscriber as _;

use crate::{
    failures::ServiceFailure,
    runtime::ServiceState,
    settings::ServiceConfiguration,
    telegram::{
        application_programming_interface::TelegramApplicationProgrammingInterfaceClient,
        worker::run_updates_loop,
    },
};

pub async fn run_service(
    service_configuration: ServiceConfiguration,
) -> Result<(), ServiceFailure> {
    let shutdown_wait_timeout_seconds = service_configuration
        .telegram_poll_timeout_seconds
        .saturating_add(2);
    let telegram_application_programming_interface_client =
        TelegramApplicationProgrammingInterfaceClient::new(
            service_configuration
                .telegram_application_programming_interface_base_uniform_resource_locator
                .clone(),
            service_configuration.telegram_bot_token.clone(),
            service_configuration.telegram_hyper_text_transfer_protocol_timeout_seconds,
        )?;
    let service_state = ServiceState::new(
        telegram_application_programming_interface_client,
        &service_configuration,
    );
    let worker_service_state = service_state.clone();
    let worker_service_configuration = Arc::new(service_configuration);
    let (shutdown_sender, shutdown_receiver) = watch::channel(false);
    let worker_shutdown_receiver = shutdown_receiver.clone();
    let mut worker_task = tokio::spawn(async move {
        run_updates_loop(
            worker_service_state,
            worker_service_configuration,
            worker_shutdown_receiver,
        )
        .await;
    });
    ctrl_c().await?;
    tracing::info!(event = "shutdown_signal_received", signal = "SIGINT");
    let _send_result = shutdown_sender.send(true);
    let worker_join_timeout_result =
        timeout(StandardDuration::from_secs(shutdown_wait_timeout_seconds), &mut worker_task).await;
    let worker_join_result = match worker_join_timeout_result {
        Ok(join_result) => join_result,
        Err(_elapsed) => {
            tracing::warn!(
                event = "graceful_shutdown_timeout",
                timeout_seconds = shutdown_wait_timeout_seconds,
                status = "forcing_abort"
            );
            worker_task.abort();
            let worker_abort_join_result = worker_task.await;
            if let Err(join_error) = worker_abort_join_result {
                if !join_error.is_cancelled() {
                    return Err(ServiceFailure::InputOutput(InputOutputError::other(format!(
                        "worker task failed after abort: {join_error}"
                    ))));
                }
            }
            return Ok(());
        }
    };
    if let Err(join_error) = worker_join_result {
        return Err(ServiceFailure::InputOutput(InputOutputError::other(format!(
            "worker task failed: {join_error}"
        ))));
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    use std::process::exit;

    use settings::ServiceConfiguration;

    drop(dotenvy::dotenv_override());
    drop(tracing_subscriber::fmt().try_init());
    let service_configuration = match ServiceConfiguration::from_env() {
        Ok(parsed_configuration) => parsed_configuration,
        Err(configuration_error) => {
            tracing::error!("startup configuration error: {configuration_error}");
            exit(1);
        }
    };
    if let Err(service_error) = run_service(service_configuration).await {
        tracing::error!("service error: {service_error}");
        exit(1);
    }
}
