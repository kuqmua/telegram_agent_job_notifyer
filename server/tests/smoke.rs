use axum as _;
use dotenvy as _;
use reqwest as _;
use serde as _;
use serde_json as _;
use server::settings::{
    ServiceConfiguration, TelegramApplicationProgrammingInterfaceBaseUniformResourceLocator,
    TelegramBotToken,
};
use telegram_agent_shared as _;
use thiserror as _;
use tokio as _;
use tracing as _;
use tracing_subscriber as _;

#[cfg(test)]
mod tests {
    use super::{
        ServiceConfiguration, TelegramApplicationProgrammingInterfaceBaseUniformResourceLocator,
        TelegramBotToken,
    };

    #[test]
    fn service_configuration_supports_telegram_only_runtime() {
        let service_configuration = ServiceConfiguration {
            polling_backoff_maximum_milliseconds: 5,
            polling_backoff_minimum_milliseconds: 1,
            polling_initial_offset: 0,
            telegram_allowed_username: None,
            telegram_application_programming_interface_base_uniform_resource_locator:
                TelegramApplicationProgrammingInterfaceBaseUniformResourceLocator::from(
                    String::from("https://api.telegram.org"),
                ),
            telegram_bot_token: TelegramBotToken::from(String::from(
                "123456789:ABCDEFGHIJKLMNOPQRSTUVWXYZ",
            )),
            telegram_chat_identifier: None,
            telegram_hyper_text_transfer_protocol_timeout_seconds: 35,
            telegram_poll_timeout_seconds: 30,
        };

        assert_eq!(
            service_configuration
                .telegram_application_programming_interface_base_uniform_resource_locator
                .as_str(),
            "https://api.telegram.org"
        );
    }
}
