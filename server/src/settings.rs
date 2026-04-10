use std::{collections::BTreeMap, env, fmt::Display, path::Path, str::FromStr};

use thiserror::Error;
const ENVIRONMENT_NAME_TELEGRAM_ALLOWED_USERNAME: &str = "TELEGRAM_ALLOWED_USERNAME";
const ENVIRONMENT_NAME_TELEGRAM_ADMIN_USERNAMES: &str = "TELEGRAM_ADMIN_USERNAMES";
const ENVIRONMENT_NAME_TELEGRAM_BOT_TOKEN: &str = "TELEGRAM_BOT_TOKEN";
const MESSAGE_VALUE_LOOKS_LIKE_A_PLACEHOLDER: &str = "value looks like a placeholder";
const MESSAGE_VALUE_MUST_BE_GREATER_THAN_ZERO: &str = "value must be greater than zero";
const MESSAGE_VALUE_MUST_NOT_BE_EMPTY: &str = "value must not be empty";
const MESSAGE_VALUE_MUST_NOT_CONTAIN_WHITESPACE: &str = "value must not contain whitespace";
const MESSAGE_SANDBOX_LAUNCHER_MUST_BE_BWRAP: &str =
    "CODEX_SANDBOX_LAUNCHER_PATH must point to bwrap executable";
const MESSAGE_SANDBOX_LAUNCHER_REQUIRED: &str =
    "CODEX_SANDBOX_LAUNCHER_PATH is required when CODEX_SANDBOX_ENABLED=true";
const MESSAGE_SANDBOX_LAUNCHER_MUST_BE_ABSOLUTE_PATH: &str =
    "CODEX_SANDBOX_LAUNCHER_PATH must be an absolute path";
const MESSAGE_SANDBOX_CUSTOM_LAUNCHER_ARGUMENTS_FORBIDDEN: &str =
    "CODEX_SANDBOX_LAUNCHER_ARGS are forbidden unless \
     CODEX_SANDBOX_ALLOW_CUSTOM_LAUNCHER_ARGS=true";
const MESSAGE_SANDBOX_WORKSPACE_ROOT_MUST_BE_ABSOLUTE_PATH: &str =
    "CODEX_SANDBOX_WORKSPACE_ROOT must be an absolute path";
const MESSAGE_SANDBOX_WORKSPACE_ROOT_REQUIRED: &str =
    "CODEX_SANDBOX_WORKSPACE_ROOT is required when CODEX_SANDBOX_ENABLED=true";
#[derive(Debug, Clone)]
pub struct ServiceConfiguration {
    pub codex_binary_path: Option<String>,
    pub codex_execution_timeout_seconds: u64,
    pub codex_max_parallel_tasks: usize,
    pub codex_output_maximum_bytes: usize,
    pub codex_sandbox_allow_custom_launcher_arguments: bool,
    pub codex_sandbox_allow_network: bool,
    pub codex_sandbox_allowed_environment_variables: Vec<String>,
    pub codex_sandbox_enabled: bool,
    pub codex_sandbox_launcher_arguments: Vec<String>,
    pub codex_sandbox_launcher_path: Option<String>,
    pub codex_sandbox_workspace_root: Option<String>,
    pub host: String,
    pub polling_backoff_max_milliseconds: u64,
    pub polling_backoff_min_milliseconds: u64,
    pub polling_initial_offset: i64,
    pub polling_timeout_seconds: u64,
    pub port: u16,
    pub processed_update_cache_size: usize,
    pub prompt_maximum_characters: usize,
    pub task_history_file_path: Option<String>,
    pub task_history_maximum_size: usize,
    pub task_list_maximum_items: usize,
    pub task_queue_max_wait_seconds: u64,
    pub task_rate_limit_per_minute: usize,
    pub telegram_admin_usernames: Vec<String>,
    pub telegram_allowed_username: Option<String>,
    pub telegram_api_base_url: String,
    pub telegram_bot_token: String,
    pub telegram_chat_identifier: Option<i64>,
    pub telegram_http_timeout_seconds: u64,
    pub telegram_message_maximum_characters: usize,
    pub update_processing_max_parallel_tasks: usize,
}
#[derive(Debug, Error, PartialEq, Eq)]
pub enum EnvironmentError {
    #[error("invalid environment variable {variable_name}: {message}")]
    InvalidEnvironmentVariable {
        message: String,
        variable_name: &'static str,
    },
    #[error("missing environment variable {variable_name}")]
    MissingEnvironmentVariable { variable_name: &'static str },
}
impl ServiceConfiguration {
    pub fn from_env() -> Result<Self, EnvironmentError> {
        Self::from_environment_map(&env::vars().collect())
    }

    pub fn from_environment_map(
        environment_variables: &BTreeMap<String, String>,
    ) -> Result<Self, EnvironmentError> {
        let telegram_bot_token = {
            let variable_name = ENVIRONMENT_NAME_TELEGRAM_BOT_TOKEN;
            let value = environment_variables
                .get(variable_name)
                .cloned()
                .ok_or(EnvironmentError::MissingEnvironmentVariable { variable_name })?;
            if value.trim().is_empty() {
                return Err(EnvironmentError::InvalidEnvironmentVariable {
                    message: String::from(MESSAGE_VALUE_MUST_NOT_BE_EMPTY),
                    variable_name,
                });
            }
            let minimum_token_length = 20usize;
            if value.len() < minimum_token_length {
                return Err(EnvironmentError::InvalidEnvironmentVariable {
                    message: format!("value must be at least {minimum_token_length} characters"),
                    variable_name,
                });
            }
            if value.chars().any(char::is_whitespace) {
                return Err(EnvironmentError::InvalidEnvironmentVariable {
                    message: String::from(MESSAGE_VALUE_MUST_NOT_CONTAIN_WHITESPACE),
                    variable_name,
                });
            }
            let lowered_case_token = value.to_ascii_lowercase();
            let suspicious_markers = ["example", "replace", "your_", "token_here"];
            if suspicious_markers
                .iter()
                .any(|suspicious_marker| lowered_case_token.contains(suspicious_marker))
            {
                return Err(EnvironmentError::InvalidEnvironmentVariable {
                    message: String::from(MESSAGE_VALUE_LOOKS_LIKE_A_PLACEHOLDER),
                    variable_name,
                });
            }
            value
        };
        let telegram_chat_identifier = environment_variables
            .get("TELEGRAM_CHAT_ID")
            .map(String::as_str)
            .map(|variable_value| parse_variable::<i64>("TELEGRAM_CHAT_ID", variable_value))
            .transpose()?;
        let telegram_allowed_username = environment_variables
            .get(ENVIRONMENT_NAME_TELEGRAM_ALLOWED_USERNAME)
            .map(String::as_str)
            .map(str::trim)
            .filter(|username_value| !username_value.is_empty())
            .map(|username_value| {
                let normalized_username = username_value
                    .strip_prefix('@')
                    .unwrap_or(username_value)
                    .to_ascii_lowercase();
                if normalized_username.is_empty() {
                    return Err(EnvironmentError::InvalidEnvironmentVariable {
                        message: String::from(MESSAGE_VALUE_MUST_NOT_BE_EMPTY),
                        variable_name: ENVIRONMENT_NAME_TELEGRAM_ALLOWED_USERNAME,
                    });
                }
                if normalized_username.chars().any(char::is_whitespace) {
                    return Err(EnvironmentError::InvalidEnvironmentVariable {
                        message: String::from(MESSAGE_VALUE_MUST_NOT_CONTAIN_WHITESPACE),
                        variable_name: ENVIRONMENT_NAME_TELEGRAM_ALLOWED_USERNAME,
                    });
                }
                Ok(normalized_username)
            })
            .transpose()?;
        let telegram_admin_usernames = environment_variables
            .get(ENVIRONMENT_NAME_TELEGRAM_ADMIN_USERNAMES)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map_or_else(Vec::new, |raw_value| {
                raw_value
                    .split(',')
                    .map(str::trim)
                    .filter(|username_value| !username_value.is_empty())
                    .map(|username_value| {
                        username_value
                            .strip_prefix('@')
                            .unwrap_or(username_value)
                            .to_ascii_lowercase()
                    })
                    .collect()
            });
        let host = environment_variables
            .get("HOST")
            .cloned()
            .unwrap_or_else(|| String::from("0.0.0.0"));
        let port = environment_variables
            .get("PORT")
            .map(String::as_str)
            .map(|variable_value| parse_variable::<u16>("PORT", variable_value))
            .transpose()?
            .unwrap_or(8080);
        let polling_timeout_seconds = environment_variables
            .get("TELEGRAM_POLL_TIMEOUT_SECONDS")
            .map(String::as_str)
            .map(|variable_value| {
                parse_positive_variable::<u64>("TELEGRAM_POLL_TIMEOUT_SECONDS", variable_value)
            })
            .transpose()?
            .unwrap_or(30);
        let polling_backoff_min_milliseconds = environment_variables
            .get("TELEGRAM_POLL_BACKOFF_MIN_MS")
            .map(String::as_str)
            .map(|variable_value| {
                parse_positive_variable::<u64>("TELEGRAM_POLL_BACKOFF_MIN_MS", variable_value)
            })
            .transpose()?
            .unwrap_or(500);
        let polling_backoff_max_milliseconds = environment_variables
            .get("TELEGRAM_POLL_BACKOFF_MAX_MS")
            .map(String::as_str)
            .map(|variable_value| {
                parse_positive_variable::<u64>("TELEGRAM_POLL_BACKOFF_MAX_MS", variable_value)
            })
            .transpose()?
            .unwrap_or(10_000);
        if polling_backoff_max_milliseconds < polling_backoff_min_milliseconds {
            return Err(EnvironmentError::InvalidEnvironmentVariable {
                message: String::from(
                    "must be greater than or equal to TELEGRAM_POLL_BACKOFF_MIN_MS",
                ),
                variable_name: "TELEGRAM_POLL_BACKOFF_MAX_MS",
            });
        }
        let polling_initial_offset = environment_variables
            .get("TELEGRAM_POLL_INITIAL_OFFSET")
            .map(String::as_str)
            .map(|variable_value| {
                parse_variable::<i64>("TELEGRAM_POLL_INITIAL_OFFSET", variable_value)
            })
            .transpose()?
            .unwrap_or(0);
        let telegram_http_timeout_seconds = environment_variables
            .get("TELEGRAM_HTTP_TIMEOUT_SECONDS")
            .map(String::as_str)
            .map(|variable_value| {
                parse_positive_variable::<u64>("TELEGRAM_HTTP_TIMEOUT_SECONDS", variable_value)
            })
            .transpose()?
            .unwrap_or(40);
        if telegram_http_timeout_seconds <= polling_timeout_seconds {
            return Err(EnvironmentError::InvalidEnvironmentVariable {
                message: String::from("must be greater than TELEGRAM_POLL_TIMEOUT_SECONDS"),
                variable_name: "TELEGRAM_HTTP_TIMEOUT_SECONDS",
            });
        }
        let telegram_api_base_url = environment_variables
            .get("TELEGRAM_API_BASE_URL")
            .cloned()
            .unwrap_or_else(|| String::from("https://api.telegram.org"));
        let codex_max_parallel_tasks = environment_variables
            .get("CODEX_MAX_PARALLEL_TASKS")
            .map(String::as_str)
            .map(|variable_value| {
                parse_positive_variable::<usize>("CODEX_MAX_PARALLEL_TASKS", variable_value)
            })
            .transpose()?
            .unwrap_or(2);
        let codex_binary_path = environment_variables
            .get("CODEX_BINARY_PATH")
            .map(String::as_str)
            .map(str::trim)
            .filter(|path_value| !path_value.is_empty())
            .map(str::to_owned);
        let codex_sandbox_allow_network = environment_variables
            .get("CODEX_SANDBOX_ALLOW_NETWORK")
            .map(String::as_str)
            .map(|variable_value| {
                parse_variable::<bool>("CODEX_SANDBOX_ALLOW_NETWORK", variable_value)
            })
            .transpose()?
            .unwrap_or(false);
        let codex_sandbox_allow_custom_launcher_arguments = environment_variables
            .get("CODEX_SANDBOX_ALLOW_CUSTOM_LAUNCHER_ARGS")
            .map(String::as_str)
            .map(|variable_value| {
                parse_variable::<bool>("CODEX_SANDBOX_ALLOW_CUSTOM_LAUNCHER_ARGS", variable_value)
            })
            .transpose()?
            .unwrap_or(false);
        let codex_sandbox_enabled = environment_variables
            .get("CODEX_SANDBOX_ENABLED")
            .map(String::as_str)
            .map(|variable_value| parse_variable::<bool>("CODEX_SANDBOX_ENABLED", variable_value))
            .transpose()?
            .unwrap_or(false);
        let codex_sandbox_workspace_root = environment_variables
            .get("CODEX_SANDBOX_WORKSPACE_ROOT")
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let codex_sandbox_launcher_path = environment_variables
            .get("CODEX_SANDBOX_LAUNCHER_PATH")
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let codex_sandbox_launcher_arguments = environment_variables
            .get("CODEX_SANDBOX_LAUNCHER_ARGS")
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map_or_else(Vec::new, |raw_value| {
                raw_value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .collect()
            });
        let codex_sandbox_allowed_environment_variables = environment_variables
            .get("CODEX_SANDBOX_ALLOWED_ENV")
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map_or_else(
                || {
                    [
                        "PATH",
                        "HOME",
                        "CODEX_HOME",
                        "OPENAI_API_KEY",
                        "HTTPS_PROXY",
                        "HTTP_PROXY",
                        "NO_PROXY",
                    ]
                    .iter()
                    .map(|value| String::from(*value))
                    .collect()
                },
                |raw_value| {
                    raw_value
                        .split(',')
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned)
                        .collect()
                },
            );
        if codex_sandbox_enabled && codex_sandbox_launcher_path.is_none() {
            return Err(EnvironmentError::InvalidEnvironmentVariable {
                message: String::from(MESSAGE_SANDBOX_LAUNCHER_REQUIRED),
                variable_name: "CODEX_SANDBOX_LAUNCHER_PATH",
            });
        }
        if codex_sandbox_enabled
            && codex_sandbox_launcher_path
                .as_deref()
                .is_some_and(|launcher_path| !launcher_path.contains("bwrap"))
        {
            return Err(EnvironmentError::InvalidEnvironmentVariable {
                message: String::from(MESSAGE_SANDBOX_LAUNCHER_MUST_BE_BWRAP),
                variable_name: "CODEX_SANDBOX_LAUNCHER_PATH",
            });
        }
        if codex_sandbox_enabled
            && codex_sandbox_launcher_path
                .as_deref()
                .is_some_and(|launcher_path| !Path::new(launcher_path).is_absolute())
        {
            return Err(EnvironmentError::InvalidEnvironmentVariable {
                message: String::from(MESSAGE_SANDBOX_LAUNCHER_MUST_BE_ABSOLUTE_PATH),
                variable_name: "CODEX_SANDBOX_LAUNCHER_PATH",
            });
        }
        if codex_sandbox_enabled && codex_sandbox_workspace_root.is_none() {
            return Err(EnvironmentError::InvalidEnvironmentVariable {
                message: String::from(MESSAGE_SANDBOX_WORKSPACE_ROOT_REQUIRED),
                variable_name: "CODEX_SANDBOX_WORKSPACE_ROOT",
            });
        }
        if codex_sandbox_enabled
            && codex_sandbox_workspace_root
                .as_deref()
                .is_some_and(|workspace_root| !Path::new(workspace_root).is_absolute())
        {
            return Err(EnvironmentError::InvalidEnvironmentVariable {
                message: String::from(MESSAGE_SANDBOX_WORKSPACE_ROOT_MUST_BE_ABSOLUTE_PATH),
                variable_name: "CODEX_SANDBOX_WORKSPACE_ROOT",
            });
        }
        if codex_sandbox_enabled
            && !codex_sandbox_allow_custom_launcher_arguments
            && !codex_sandbox_launcher_arguments.is_empty()
        {
            return Err(EnvironmentError::InvalidEnvironmentVariable {
                message: String::from(MESSAGE_SANDBOX_CUSTOM_LAUNCHER_ARGUMENTS_FORBIDDEN),
                variable_name: "CODEX_SANDBOX_LAUNCHER_ARGS",
            });
        }
        let codex_execution_timeout_seconds = environment_variables
            .get("CODEX_TIMEOUT_SECONDS")
            .map(String::as_str)
            .map(|variable_value| {
                parse_positive_variable::<u64>("CODEX_TIMEOUT_SECONDS", variable_value)
            })
            .transpose()?
            .unwrap_or(120);
        let codex_output_maximum_bytes = environment_variables
            .get("CODEX_OUTPUT_MAX_BYTES")
            .map(String::as_str)
            .map(|variable_value| {
                parse_positive_variable::<usize>("CODEX_OUTPUT_MAX_BYTES", variable_value)
            })
            .transpose()?
            .unwrap_or(65_536);
        let telegram_message_maximum_characters = environment_variables
            .get("TELEGRAM_MESSAGE_MAX_CHARACTERS")
            .map(String::as_str)
            .map(|variable_value| {
                parse_positive_variable::<usize>("TELEGRAM_MESSAGE_MAX_CHARACTERS", variable_value)
            })
            .transpose()?
            .unwrap_or(3_500);
        let processed_update_cache_size = environment_variables
            .get("PROCESSED_UPDATE_CACHE_SIZE")
            .map(String::as_str)
            .map(|variable_value| {
                parse_positive_variable::<usize>("PROCESSED_UPDATE_CACHE_SIZE", variable_value)
            })
            .transpose()?
            .unwrap_or(4_096);
        let prompt_maximum_characters = environment_variables
            .get("PROMPT_MAX_CHARACTERS")
            .map(String::as_str)
            .map(|variable_value| {
                parse_positive_variable::<usize>("PROMPT_MAX_CHARACTERS", variable_value)
            })
            .transpose()?
            .unwrap_or(8_000);
        let update_processing_max_parallel_tasks = environment_variables
            .get("UPDATE_MAX_PARALLEL_TASKS")
            .map(String::as_str)
            .map(|variable_value| {
                parse_positive_variable::<usize>("UPDATE_MAX_PARALLEL_TASKS", variable_value)
            })
            .transpose()?
            .unwrap_or(64);
        let task_history_file_path = environment_variables
            .get("TASK_HISTORY_FILE_PATH")
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let task_history_maximum_size = environment_variables
            .get("TASK_HISTORY_MAX_SIZE")
            .map(String::as_str)
            .map(|variable_value| {
                parse_positive_variable::<usize>("TASK_HISTORY_MAX_SIZE", variable_value)
            })
            .transpose()?
            .unwrap_or(2_048);
        let task_rate_limit_per_minute = environment_variables
            .get("TASK_RATE_LIMIT_PER_MINUTE")
            .map(String::as_str)
            .map(|variable_value| {
                parse_positive_variable::<usize>("TASK_RATE_LIMIT_PER_MINUTE", variable_value)
            })
            .transpose()?
            .unwrap_or(30);
        let task_list_maximum_items = environment_variables
            .get("TASK_LIST_MAX_ITEMS")
            .map(String::as_str)
            .map(|variable_value| {
                parse_positive_variable::<usize>("TASK_LIST_MAX_ITEMS", variable_value)
            })
            .transpose()?
            .unwrap_or(10);
        let task_queue_max_wait_seconds = environment_variables
            .get("TASK_QUEUE_MAX_WAIT_SECONDS")
            .map(String::as_str)
            .map(|variable_value| {
                parse_positive_variable::<u64>("TASK_QUEUE_MAX_WAIT_SECONDS", variable_value)
            })
            .transpose()?
            .unwrap_or(120);
        Ok(Self {
            codex_binary_path,
            codex_execution_timeout_seconds,
            codex_max_parallel_tasks,
            codex_output_maximum_bytes,
            codex_sandbox_allow_custom_launcher_arguments,
            codex_sandbox_allow_network,
            codex_sandbox_allowed_environment_variables,
            codex_sandbox_enabled,
            codex_sandbox_launcher_arguments,
            codex_sandbox_launcher_path,
            codex_sandbox_workspace_root,
            host,
            polling_backoff_max_milliseconds,
            polling_backoff_min_milliseconds,
            polling_initial_offset,
            polling_timeout_seconds,
            port,
            processed_update_cache_size,
            prompt_maximum_characters,
            task_history_file_path,
            task_history_maximum_size,
            task_list_maximum_items,
            task_queue_max_wait_seconds,
            task_rate_limit_per_minute,
            telegram_admin_usernames,
            telegram_allowed_username,
            telegram_api_base_url,
            telegram_bot_token,
            telegram_chat_identifier,
            telegram_http_timeout_seconds,
            telegram_message_maximum_characters,
            update_processing_max_parallel_tasks,
        })
    }
}
fn parse_positive_variable<Value>(
    variable_name: &'static str,
    variable_value: &str,
) -> Result<Value, EnvironmentError>
where
    Value: FromStr + PartialEq + From<u8>,
    <Value as FromStr>::Err: Display,
{
    let parsed_value = parse_variable::<Value>(variable_name, variable_value)?;
    if parsed_value == Value::from(0) {
        return Err(EnvironmentError::InvalidEnvironmentVariable {
            message: String::from(MESSAGE_VALUE_MUST_BE_GREATER_THAN_ZERO),
            variable_name,
        });
    }
    Ok(parsed_value)
}
fn parse_variable<Value>(
    variable_name: &'static str,
    variable_value: &str,
) -> Result<Value, EnvironmentError>
where
    Value: FromStr,
    <Value as FromStr>::Err: Display,
{
    variable_value.parse::<Value>().map_err(|parse_error| {
        EnvironmentError::InvalidEnvironmentVariable {
            message: parse_error.to_string(),
            variable_name,
        }
    })
}
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{EnvironmentError, ServiceConfiguration};
    fn base_environment() -> BTreeMap<String, String> {
        BTreeMap::from([
            (
                String::from("TELEGRAM_BOT_TOKEN"),
                String::from("123456789:ABCDEFGHIJKLMNOPQRSTUVWXYZ"),
            ),
            (String::from("HOST"), String::from("127.0.0.1")),
            (String::from("PORT"), String::from("8080")),
        ])
    }
    #[test]
    fn from_environment_map_parses_defaults() {
        let parsed_settings =
            ServiceConfiguration::from_environment_map(&base_environment()).expect("a4c2f8d1");
        assert!(!parsed_settings.codex_sandbox_allow_custom_launcher_arguments);
        assert!(!parsed_settings.codex_sandbox_allow_network);
        assert_eq!(parsed_settings.codex_max_parallel_tasks, 2);
        assert!(!parsed_settings.codex_sandbox_enabled);
        assert_eq!(parsed_settings.codex_sandbox_allowed_environment_variables, vec![
            String::from("PATH"),
            String::from("HOME"),
            String::from("CODEX_HOME"),
            String::from("OPENAI_API_KEY"),
            String::from("HTTPS_PROXY"),
            String::from("HTTP_PROXY"),
            String::from("NO_PROXY"),
        ]);
        assert_eq!(parsed_settings.polling_backoff_max_milliseconds, 10_000);
        assert_eq!(parsed_settings.polling_backoff_min_milliseconds, 500);
        assert_eq!(parsed_settings.polling_timeout_seconds, 30);
        assert_eq!(parsed_settings.prompt_maximum_characters, 8_000);
        assert_eq!(parsed_settings.task_queue_max_wait_seconds, 120);
        assert_eq!(parsed_settings.telegram_http_timeout_seconds, 40);
        assert_eq!(parsed_settings.update_processing_max_parallel_tasks, 64);
        assert_eq!(parsed_settings.codex_binary_path, None);
        assert_eq!(parsed_settings.telegram_allowed_username, None);
    }
    #[test]
    fn from_environment_map_rejects_placeholder_token() {
        let mut environment_variables = base_environment();
        let _previous_value = environment_variables.insert(
            String::from("TELEGRAM_BOT_TOKEN"),
            String::from("replace_with_your_token_here"),
        );
        let parsed_settings_result =
            ServiceConfiguration::from_environment_map(&environment_variables);
        let _error = parsed_settings_result.expect_err("ef7a12b9");
    }
    #[test]
    fn from_environment_map_reports_missing_token() {
        let parsed_settings_result = ServiceConfiguration::from_environment_map(&BTreeMap::new());
        assert!(matches!(
            parsed_settings_result,
            Err(EnvironmentError::MissingEnvironmentVariable {
                variable_name: super::ENVIRONMENT_NAME_TELEGRAM_BOT_TOKEN
            })
        ));
    }
    #[test]
    fn from_environment_map_parses_codex_binary_path() {
        let mut environment_variables = base_environment();
        let _previous_value = environment_variables
            .insert(String::from("CODEX_BINARY_PATH"), String::from("/usr/local/bin/codex"));
        let parsed_settings =
            ServiceConfiguration::from_environment_map(&environment_variables).expect("f2d5a8c1");
        assert_eq!(parsed_settings.codex_binary_path.as_deref(), Some("/usr/local/bin/codex"));
    }

    #[test]
    fn from_environment_map_parses_codex_sandbox_configuration() {
        let mut environment_variables = base_environment();
        let _previous_enabled_value = environment_variables
            .insert(String::from("CODEX_SANDBOX_ENABLED"), String::from("true"));
        let _previous_workspace_value = environment_variables.insert(
            String::from("CODEX_SANDBOX_WORKSPACE_ROOT"),
            String::from("/tmp/codex-sandbox"),
        );
        let _previous_launcher_path_value = environment_variables
            .insert(String::from("CODEX_SANDBOX_LAUNCHER_PATH"), String::from("/usr/bin/bwrap"));
        let _previous_launcher_arguments_value = environment_variables.insert(
            String::from("CODEX_SANDBOX_LAUNCHER_ARGS"),
            String::from("--unshare-net,--ro-bind,/usr,/usr"),
        );
        let _previous_allow_custom_arguments_value = environment_variables
            .insert(String::from("CODEX_SANDBOX_ALLOW_CUSTOM_LAUNCHER_ARGS"), String::from("true"));
        let _previous_allow_network_value = environment_variables
            .insert(String::from("CODEX_SANDBOX_ALLOW_NETWORK"), String::from("true"));
        let _previous_allowed_environment_value = environment_variables
            .insert(String::from("CODEX_SANDBOX_ALLOWED_ENV"), String::from("PATH,OPENAI_API_KEY"));
        let parsed_settings =
            ServiceConfiguration::from_environment_map(&environment_variables).expect("e2f4a6c8");
        assert!(parsed_settings.codex_sandbox_enabled);
        assert!(parsed_settings.codex_sandbox_allow_custom_launcher_arguments);
        assert!(parsed_settings.codex_sandbox_allow_network);
        assert_eq!(
            parsed_settings.codex_sandbox_workspace_root.as_deref(),
            Some("/tmp/codex-sandbox")
        );
        assert_eq!(parsed_settings.codex_sandbox_launcher_path.as_deref(), Some("/usr/bin/bwrap"));
        assert_eq!(parsed_settings.codex_sandbox_launcher_arguments, vec![
            String::from("--unshare-net"),
            String::from("--ro-bind"),
            String::from("/usr"),
            String::from("/usr"),
        ]);
        assert_eq!(parsed_settings.codex_sandbox_allowed_environment_variables, vec![
            String::from("PATH"),
            String::from("OPENAI_API_KEY")
        ]);
    }

    #[test]
    fn from_environment_map_rejects_enabled_sandbox_without_launcher() {
        let mut environment_variables = base_environment();
        let _previous_enabled_value = environment_variables
            .insert(String::from("CODEX_SANDBOX_ENABLED"), String::from("true"));
        let _previous_workspace_value = environment_variables.insert(
            String::from("CODEX_SANDBOX_WORKSPACE_ROOT"),
            String::from("/tmp/codex-sandbox"),
        );
        let parsed_settings_result =
            ServiceConfiguration::from_environment_map(&environment_variables);
        assert!(matches!(
            parsed_settings_result,
            Err(EnvironmentError::InvalidEnvironmentVariable {
                variable_name: "CODEX_SANDBOX_LAUNCHER_PATH",
                ..
            })
        ));
    }

    #[test]
    fn from_environment_map_rejects_enabled_sandbox_without_workspace_root() {
        let mut environment_variables = base_environment();
        let _previous_enabled_value = environment_variables
            .insert(String::from("CODEX_SANDBOX_ENABLED"), String::from("true"));
        let _previous_launcher_path_value = environment_variables
            .insert(String::from("CODEX_SANDBOX_LAUNCHER_PATH"), String::from("/usr/bin/bwrap"));
        let parsed_settings_result =
            ServiceConfiguration::from_environment_map(&environment_variables);
        assert!(matches!(
            parsed_settings_result,
            Err(EnvironmentError::InvalidEnvironmentVariable {
                variable_name: "CODEX_SANDBOX_WORKSPACE_ROOT",
                ..
            })
        ));
    }

    #[test]
    fn from_environment_map_rejects_enabled_sandbox_with_non_bwrap_launcher() {
        let mut environment_variables = base_environment();
        let _previous_enabled_value = environment_variables
            .insert(String::from("CODEX_SANDBOX_ENABLED"), String::from("true"));
        let _previous_workspace_value = environment_variables.insert(
            String::from("CODEX_SANDBOX_WORKSPACE_ROOT"),
            String::from("/tmp/codex-sandbox"),
        );
        let _previous_launcher_path_value = environment_variables
            .insert(String::from("CODEX_SANDBOX_LAUNCHER_PATH"), String::from("/usr/bin/firejail"));
        let parsed_settings_result =
            ServiceConfiguration::from_environment_map(&environment_variables);
        assert!(matches!(
            parsed_settings_result,
            Err(EnvironmentError::InvalidEnvironmentVariable {
                variable_name: "CODEX_SANDBOX_LAUNCHER_PATH",
                ..
            })
        ));
    }

    #[test]
    fn from_environment_map_rejects_enabled_sandbox_with_non_absolute_launcher_path() {
        let mut environment_variables = base_environment();
        let _previous_enabled_value = environment_variables
            .insert(String::from("CODEX_SANDBOX_ENABLED"), String::from("true"));
        let _previous_workspace_value = environment_variables.insert(
            String::from("CODEX_SANDBOX_WORKSPACE_ROOT"),
            String::from("/tmp/codex-sandbox"),
        );
        let _previous_launcher_path_value = environment_variables
            .insert(String::from("CODEX_SANDBOX_LAUNCHER_PATH"), String::from("usr/bin/bwrap"));
        let parsed_settings_result =
            ServiceConfiguration::from_environment_map(&environment_variables);
        assert!(matches!(
            parsed_settings_result,
            Err(EnvironmentError::InvalidEnvironmentVariable {
                variable_name: "CODEX_SANDBOX_LAUNCHER_PATH",
                ..
            })
        ));
    }

    #[test]
    fn from_environment_map_rejects_enabled_sandbox_with_non_absolute_workspace_root() {
        let mut environment_variables = base_environment();
        let _previous_enabled_value = environment_variables
            .insert(String::from("CODEX_SANDBOX_ENABLED"), String::from("true"));
        let _previous_workspace_value = environment_variables.insert(
            String::from("CODEX_SANDBOX_WORKSPACE_ROOT"),
            String::from("tmp/codex-sandbox"),
        );
        let _previous_launcher_path_value = environment_variables
            .insert(String::from("CODEX_SANDBOX_LAUNCHER_PATH"), String::from("/usr/bin/bwrap"));
        let parsed_settings_result =
            ServiceConfiguration::from_environment_map(&environment_variables);
        assert!(matches!(
            parsed_settings_result,
            Err(EnvironmentError::InvalidEnvironmentVariable {
                variable_name: "CODEX_SANDBOX_WORKSPACE_ROOT",
                ..
            })
        ));
    }

    #[test]
    fn from_environment_map_rejects_custom_sandbox_launcher_arguments_by_default() {
        let mut environment_variables = base_environment();
        let _previous_enabled_value = environment_variables
            .insert(String::from("CODEX_SANDBOX_ENABLED"), String::from("true"));
        let _previous_workspace_value = environment_variables.insert(
            String::from("CODEX_SANDBOX_WORKSPACE_ROOT"),
            String::from("/tmp/codex-sandbox"),
        );
        let _previous_launcher_path_value = environment_variables
            .insert(String::from("CODEX_SANDBOX_LAUNCHER_PATH"), String::from("/usr/bin/bwrap"));
        let _previous_launcher_arguments_value = environment_variables
            .insert(String::from("CODEX_SANDBOX_LAUNCHER_ARGS"), String::from("--share-net"));
        let parsed_settings_result =
            ServiceConfiguration::from_environment_map(&environment_variables);
        assert!(matches!(
            parsed_settings_result,
            Err(EnvironmentError::InvalidEnvironmentVariable {
                variable_name: "CODEX_SANDBOX_LAUNCHER_ARGS",
                ..
            })
        ));
    }
    #[test]
    fn from_environment_map_rejects_non_greater_http_timeout_for_polling() {
        let mut environment_variables = base_environment();
        let _previous_poll_timeout_value = environment_variables
            .insert(String::from("TELEGRAM_POLL_TIMEOUT_SECONDS"), String::from("30"));
        let _previous_http_timeout_value = environment_variables
            .insert(String::from("TELEGRAM_HTTP_TIMEOUT_SECONDS"), String::from("30"));
        let parsed_settings_result =
            ServiceConfiguration::from_environment_map(&environment_variables);
        assert!(matches!(
            parsed_settings_result,
            Err(EnvironmentError::InvalidEnvironmentVariable {
                variable_name: "TELEGRAM_HTTP_TIMEOUT_SECONDS",
                ..
            })
        ));
    }
    #[test]
    fn from_environment_map_parses_telegram_allowed_username_with_at_prefix() {
        let mut environment_variables = base_environment();
        let _previous_value = environment_variables
            .insert(String::from("TELEGRAM_ALLOWED_USERNAME"), String::from("@Kuqmua"));
        let parsed_settings =
            ServiceConfiguration::from_environment_map(&environment_variables).expect("cb9a12e4");
        assert_eq!(parsed_settings.telegram_allowed_username.as_deref(), Some("kuqmua"));
    }
}
