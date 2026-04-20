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
        let telegram_bot_token =
            parse_non_empty_string(ENVIRONMENT_NAME_TELEGRAM_BOT_TOKEN, &telegram_bot_token_raw)?;
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
        if telegram_hyper_text_transfer_protocol_timeout_seconds <= telegram_poll_timeout_seconds {
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

fn parse_i64(variable_name: &'static str, variable_value: &str) -> Result<i64, EnvironmentError> {
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
