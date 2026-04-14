use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const DEFAULT_DEEPSEEK_CHAT_COMPLETION_URL: &str = "https://api.deepseek.com/chat/completions";
const DEFAULT_DEEPSEEK_MODEL: &str = "deepseek-chat";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeepseekExecutionConfiguration<'configuration> {
    pub api_key: &'configuration str,
    pub api_url: &'configuration str,
    pub model: &'configuration str,
    pub system_prompt: Option<&'configuration str>,
}

impl Default for DeepseekExecutionConfiguration<'static> {
    fn default() -> Self {
        Self {
            api_key: "",
            api_url: DEFAULT_DEEPSEEK_CHAT_COMPLETION_URL,
            model: DEFAULT_DEEPSEEK_MODEL,
            system_prompt: None,
        }
    }
}

#[derive(Debug, Error)]
pub enum DeepseekExecutionError {
    #[error("deepseek api error ({status_code}): {message}")]
    ApiError { message: String, status_code: u16 },
    #[error("deepseek http transport failed: {source}")]
    HttpTransport {
        #[from]
        source: reqwest::Error,
    },
    #[error("invalid configuration: {message}")]
    InvalidConfiguration { message: String },
    #[error("invalid deepseek response: {message}")]
    InvalidResponse { message: String },
}

#[derive(Debug, Deserialize)]
struct DeepseekErrorEnvelope {
    error: DeepseekErrorPayload,
}

#[derive(Debug, Deserialize)]
struct DeepseekErrorPayload {
    message: String,
}

#[derive(Debug, Deserialize)]
struct DeepseekResponseEnvelope {
    choices: Vec<DeepseekChoice>,
}

#[derive(Debug, Deserialize)]
struct DeepseekChoice {
    message: DeepseekResponseMessage,
}

#[derive(Debug, Deserialize)]
struct DeepseekResponseMessage {
    content: String,
}

#[derive(Debug, Serialize)]
struct DeepseekChatCompletionRequest<'request> {
    messages: Vec<DeepseekRequestMessage<'request>>,
    model: &'request str,
}

#[derive(Debug, Serialize)]
struct DeepseekRequestMessage<'request> {
    content: &'request str,
    role: &'request str,
}

pub async fn exec_prompt(prompt: &str, api_key: &str) -> Result<String, DeepseekExecutionError> {
    let configuration = DeepseekExecutionConfiguration {
        api_key,
        ..DeepseekExecutionConfiguration::default()
    };
    exec_prompt_with_configuration(prompt, configuration).await
}

pub async fn exec_prompt_with_configuration(
    prompt: &str,
    configuration: DeepseekExecutionConfiguration<'_>,
) -> Result<String, DeepseekExecutionError> {
    validate_prompt_and_configuration(prompt, configuration)?;

    let mut messages = Vec::with_capacity(2);
    if let Some(system_message_content) = configuration
        .system_prompt
        .filter(|content| !content.trim().is_empty())
    {
        messages.push(DeepseekRequestMessage {
            content: system_message_content,
            role: "system",
        });
    }
    messages.push(DeepseekRequestMessage {
        content: prompt,
        role: "user",
    });
    let request_payload = DeepseekChatCompletionRequest {
        messages,
        model: configuration.model,
    };

    let response = Client::new()
        .post(configuration.api_url)
        .bearer_auth(configuration.api_key)
        .json(&request_payload)
        .send()
        .await?;

    let status_code = response.status().as_u16();
    let response_body = response.text().await?;

    if status_code >= 400 {
        let api_error_message = serde_json::from_str::<DeepseekErrorEnvelope>(&response_body)
            .ok()
            .map(|error_envelope| error_envelope.error.message)
            .filter(|message| !message.trim().is_empty())
            .unwrap_or_else(|| format!("deepseek request failed with status code {status_code}"));
        return Err(DeepseekExecutionError::ApiError {
            message: api_error_message,
            status_code,
        });
    }

    let completion_content = serde_json::from_str::<DeepseekResponseEnvelope>(&response_body)
        .ok()
        .and_then(|completion_envelope| {
            completion_envelope
                .choices
                .first()
                .map(|choice| choice.message.content.clone())
                .filter(|content| !content.trim().is_empty())
        });
    completion_content.ok_or_else(|| DeepseekExecutionError::InvalidResponse {
        message: String::from("response does not contain completion text"),
    })
}

pub fn validate_prompt_and_configuration(
    prompt: &str,
    configuration: DeepseekExecutionConfiguration<'_>,
) -> Result<(), DeepseekExecutionError> {
    if prompt.trim().is_empty() {
        return Err(DeepseekExecutionError::InvalidConfiguration {
            message: String::from("prompt must not be empty"),
        });
    }
    if configuration.api_key.trim().is_empty() {
        return Err(DeepseekExecutionError::InvalidConfiguration {
            message: String::from("api key must not be empty"),
        });
    }
    if configuration.api_url.trim().is_empty() {
        return Err(DeepseekExecutionError::InvalidConfiguration {
            message: String::from("api url must not be empty"),
        });
    }
    if configuration.model.trim().is_empty() {
        return Err(DeepseekExecutionError::InvalidConfiguration {
            message: String::from("model must not be empty"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        DeepseekExecutionConfiguration, DeepseekExecutionError, validate_prompt_and_configuration,
    };

    #[test]
    fn validate_prompt_and_configuration_rejects_empty_prompt() {
        let configuration = DeepseekExecutionConfiguration {
            api_key: "key",
            ..DeepseekExecutionConfiguration::default()
        };

        let validation_result = validate_prompt_and_configuration("   ", configuration);

        assert!(matches!(
            validation_result,
            Err(DeepseekExecutionError::InvalidConfiguration { .. })
        ));
    }

    #[test]
    fn validate_prompt_and_configuration_rejects_empty_api_key() {
        let configuration = DeepseekExecutionConfiguration {
            api_key: "   ",
            ..DeepseekExecutionConfiguration::default()
        };

        let validation_result = validate_prompt_and_configuration("hello", configuration);

        assert!(matches!(
            validation_result,
            Err(DeepseekExecutionError::InvalidConfiguration { .. })
        ));
    }

    #[test]
    fn validate_prompt_and_configuration_accepts_valid_configuration() {
        let configuration = DeepseekExecutionConfiguration {
            api_key: "key",
            ..DeepseekExecutionConfiguration::default()
        };

        let validation_result = validate_prompt_and_configuration("hello", configuration);

        assert!(matches!(validation_result, Ok(())));
    }
}
