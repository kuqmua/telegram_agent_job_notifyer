use axum as _;
use dotenvy as _;
use reqwest as _;
use serde as _;
use serde_json as _;
use telegram_agent_shared as _;
use thiserror as _;
use tracing as _;
use tracing_subscriber as _;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        Json, Router,
        extract::{Path, State},
        routing::{get, post},
    };
    use serde_json::{Value, json};
    use server::{
        runtime::ServiceState,
        settings::{
            ServiceConfiguration,
            TelegramApplicationProgrammingInterfaceBaseUniformResourceLocator, TelegramBotToken,
        },
        shared::ChatIdentifier,
        telegram::{
            application_programming_interface::TelegramApplicationProgrammingInterfaceClient,
            worker::run_updates_loop,
        },
    };
    use tokio::{
        net::TcpListener,
        sync::{Mutex, watch},
        time::{Duration, sleep},
    };

    #[derive(Clone, Default)]
    struct MockTelegramState {
        sent_messages: Arc<Mutex<Vec<String>>>,
    }

    #[tokio::test]
    async fn worker_processes_health_command() {
        let mock_telegram_state = MockTelegramState::default();
        let mock_application = Router::new()
            .route(
                "/bot{token}/getUpdates",
                get(async |Path(_token): Path<String>| {
                    Json(json!({
                        "ok": true,
                        "result": [
                            {
                                "update_id": 1i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "from": { "username": "telegram_user" },
                                    "text": "/health"
                                }
                            }
                        ]
                    }))
                }),
            )
            .route(
                "/bot{token}/sendMessage",
                post(
                    async |Path(_token): Path<String>,
                           State(route_state): State<MockTelegramState>,
                           Json(payload): Json<Value>| {
                        if let Some(message_text) = payload.get("text").and_then(Value::as_str) {
                            route_state
                                .sent_messages
                                .lock()
                                .await
                                .push(String::from(message_text));
                        }
                        Json(json!({ "ok": true, "result": {} }))
                    },
                ),
            )
            .with_state(mock_telegram_state.clone());

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("1a2b3c4d");
        let bound_address = listener.local_addr().expect("2b3c4d5e");
        let server_task = tokio::spawn(async move {
            drop(axum::serve(listener, mock_application).await);
        });

        let service_configuration = ServiceConfiguration {
            polling_backoff_maximum_milliseconds: 5,
            polling_backoff_minimum_milliseconds: 1,
            polling_initial_offset: 0,
            telegram_allowed_username: None,
            telegram_application_programming_interface_base_uniform_resource_locator:
                TelegramApplicationProgrammingInterfaceBaseUniformResourceLocator::from(format!(
                    "http://{bound_address}"
                )),
            telegram_bot_token: TelegramBotToken::from(String::from("test-token")),
            telegram_chat_identifier: Some(ChatIdentifier::from(111)),
            telegram_hyper_text_transfer_protocol_timeout_seconds: 5,
            telegram_poll_timeout_seconds: 1,
        };
        let telegram_client = TelegramApplicationProgrammingInterfaceClient::new(
            service_configuration
                .telegram_application_programming_interface_base_uniform_resource_locator
                .clone(),
            service_configuration.telegram_bot_token.clone(),
            service_configuration.telegram_hyper_text_transfer_protocol_timeout_seconds,
        )
        .expect("3c4d5e6f");
        let service_state = ServiceState::new(telegram_client, &service_configuration);
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);

        let worker_task = tokio::spawn(run_updates_loop(
            service_state,
            Arc::new(service_configuration),
            shutdown_receiver,
        ));
        sleep(Duration::from_millis(250)).await;

        let _send_result = shutdown_sender.send(true);
        worker_task.abort();
        server_task.abort();

        let first_message = {
            let sent_messages = mock_telegram_state.sent_messages.lock().await;
            sent_messages.first().map(String::as_str).map(String::from)
        };
        assert_eq!(first_message.as_deref(), Some("[telegram-agent] Health check: bot is alive"));
    }
}
