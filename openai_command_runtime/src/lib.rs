use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const DEFAULT_OPENAI_CHAT_COMPLETION_UNIFORM_RESOURCE_LOCATOR: &str =
    "https://api.openai.com/v1/chat/completions";
const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpenaiExecutionConfiguration<'configuration> {
    pub application_programming_interface_key: &'configuration str,
    pub application_programming_interface_uniform_resource_locator: &'configuration str,
    pub model: &'configuration str,
    pub system_prompt: Option<&'configuration str>,
}

impl Default for OpenaiExecutionConfiguration<'static> {
    fn default() -> Self {
        Self {
            application_programming_interface_key: "",
            application_programming_interface_uniform_resource_locator:
                DEFAULT_OPENAI_CHAT_COMPLETION_UNIFORM_RESOURCE_LOCATOR,
            model: DEFAULT_OPENAI_MODEL,
            system_prompt: None,
        }
    }
}

#[derive(Debug, Error)]
pub enum OpenaiExecutionError {
    #[error("openai api error ({status_code}): {message}")]
    ApplicationProgrammingInterfaceError { message: String, status_code: u16 },
    #[error("openai http transport failed: {source}")]
    HyperTextTransferProtocolTransport {
        #[from]
        source: reqwest::Error,
    },
    #[error("invalid configuration: {message}")]
    InvalidConfiguration { message: String },
    #[error("invalid openai response: {message}")]
    InvalidResponse { message: String },
}

#[derive(Debug, Deserialize)]
struct OpenaiErrorEnvelope {
    error: OpenaiErrorPayload,
}

#[derive(Debug, Deserialize)]
struct OpenaiErrorPayload {
    message: String,
}

#[derive(Debug, Deserialize)]
struct OpenaiResponseEnvelope {
    choices: Vec<OpenaiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenaiChoice {
    message: OpenaiResponseMessage,
}

#[derive(Debug, Deserialize)]
struct OpenaiResponseMessage {
    content: String,
}

#[derive(Debug, Serialize)]
struct OpenaiChatCompletionRequest<'request> {
    messages: Vec<OpenaiRequestMessage<'request>>,
    model: &'request str,
}

#[derive(Debug, Serialize)]
struct OpenaiRequestMessage<'request> {
    content: &'request str,
    role: &'request str,
}

pub async fn exec_prompt(
    prompt: &str,
    application_programming_interface_key: &str,
) -> Result<String, OpenaiExecutionError> {
    let configuration = OpenaiExecutionConfiguration {
        application_programming_interface_key,
        ..OpenaiExecutionConfiguration::default()
    };
    exec_prompt_with_configuration(prompt, configuration).await
}

pub async fn exec_prompt_with_configuration(
    prompt: &str,
    configuration: OpenaiExecutionConfiguration<'_>,
) -> Result<String, OpenaiExecutionError> {
    validate_prompt_and_configuration(prompt, configuration)?;

    let mut messages = Vec::with_capacity(2);
    if let Some(system_message_content) = configuration
        .system_prompt
        .filter(|content| !content.trim().is_empty())
    {
        messages.push(OpenaiRequestMessage {
            content: system_message_content,
            role: "system",
        });
    }
    messages.push(OpenaiRequestMessage {
        content: prompt,
        role: "user",
    });
    let request_payload = OpenaiChatCompletionRequest {
        messages,
        model: configuration.model,
    };

    let response = Client::new()
        .post(configuration.application_programming_interface_uniform_resource_locator)
        .bearer_auth(configuration.application_programming_interface_key)
        .json(&request_payload)
        .send()
        .await?;

    let status_code = response.status().as_u16();
    let response_body = response.text().await?;

    if status_code >= 400 {
        let application_programming_interface_error_message =
            serde_json::from_str::<OpenaiErrorEnvelope>(&response_body)
                .ok()
                .map(|error_envelope| error_envelope.error.message)
                .filter(|message| !message.trim().is_empty())
                .unwrap_or_else(|| format!("openai request failed with status code {status_code}"));
        return Err(OpenaiExecutionError::ApplicationProgrammingInterfaceError {
            message: application_programming_interface_error_message,
            status_code,
        });
    }

    let completion_content = serde_json::from_str::<OpenaiResponseEnvelope>(&response_body)
        .ok()
        .and_then(|completion_envelope| {
            completion_envelope
                .choices
                .first()
                .map(|choice| choice.message.content.clone())
                .filter(|content| !content.trim().is_empty())
        });
    completion_content.ok_or_else(|| OpenaiExecutionError::InvalidResponse {
        message: String::from("response does not contain completion text"),
    })
}

pub fn validate_prompt_and_configuration(
    prompt: &str,
    configuration: OpenaiExecutionConfiguration<'_>,
) -> Result<(), OpenaiExecutionError> {
    if prompt.trim().is_empty() {
        return Err(OpenaiExecutionError::InvalidConfiguration {
            message: String::from("prompt must not be empty"),
        });
    }
    if configuration
        .application_programming_interface_key
        .trim()
        .is_empty()
    {
        return Err(OpenaiExecutionError::InvalidConfiguration {
            message: String::from("application programming interface key must not be empty"),
        });
    }
    if configuration
        .application_programming_interface_uniform_resource_locator
        .trim()
        .is_empty()
    {
        return Err(OpenaiExecutionError::InvalidConfiguration {
            message: String::from(
                "application programming interface uniform resource locator must not be empty",
            ),
        });
    }
    if configuration.model.trim().is_empty() {
        return Err(OpenaiExecutionError::InvalidConfiguration {
            message: String::from("model must not be empty"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        OpenaiExecutionConfiguration, OpenaiExecutionError, validate_prompt_and_configuration,
    };

    #[test]
    fn validate_prompt_and_configuration_rejects_empty_prompt() {
        let configuration = OpenaiExecutionConfiguration {
            application_programming_interface_key: "key",
            ..OpenaiExecutionConfiguration::default()
        };

        let validation_result = validate_prompt_and_configuration("   ", configuration);

        assert!(matches!(
            validation_result,
            Err(OpenaiExecutionError::InvalidConfiguration { .. })
        ));
    }

    #[test]
    fn validate_prompt_and_configuration_rejects_empty_application_programming_interface_key() {
        let configuration = OpenaiExecutionConfiguration {
            application_programming_interface_key: "   ",
            ..OpenaiExecutionConfiguration::default()
        };

        let validation_result = validate_prompt_and_configuration("hello", configuration);

        assert!(matches!(
            validation_result,
            Err(OpenaiExecutionError::InvalidConfiguration { .. })
        ));
    }

    #[test]
    fn validate_prompt_and_configuration_accepts_valid_configuration() {
        let configuration = OpenaiExecutionConfiguration {
            application_programming_interface_key: "key",
            ..OpenaiExecutionConfiguration::default()
        };

        let validation_result = validate_prompt_and_configuration("hello", configuration);

        assert!(matches!(validation_result, Ok(())));
    }
}
