use std::time::Duration;

use reqwest::{Client, StatusCode};
use thiserror::Error;

use crate::telegram::model::{
    TelegramGetUpdatesResponse, TelegramSendMessageRequest, TelegramSendMessageResponse,
    TelegramUpdate,
};

#[derive(Clone, Debug)]
pub struct TelegramApiClient {
    api_base_url: String,
    bot_token: String,
    http_client: Client,
}

#[derive(Debug, Error)]
pub enum TelegramApiError {
    #[error("telegram api returned error: {0}")]
    ApiReported(String),
    #[error("telegram http status {status_code}: {response_body}")]
    HttpStatus {
        response_body: String,
        status_code: StatusCode,
    },
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
}

impl TelegramApiError {
    #[must_use]
    pub fn is_temporary(&self) -> bool {
        match self {
            Self::ApiReported(_) => false,
            Self::HttpStatus { status_code, .. } => {
                *status_code == StatusCode::TOO_MANY_REQUESTS || status_code.is_server_error()
            }
            Self::Request(request_error) => {
                request_error.is_connect()
                    || request_error.is_request()
                    || request_error.is_timeout()
            }
        }
    }
}

impl TelegramApiClient {
    pub async fn get_updates(
        &self,
        update_offset: i64,
        timeout_seconds: u64,
    ) -> Result<Vec<TelegramUpdate>, TelegramApiError> {
        let request_url = format!("{}/bot{}/getUpdates", self.api_base_url, self.bot_token);
        let response = self
            .http_client
            .get(request_url)
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
            return Err(TelegramApiError::HttpStatus {
                response_body,
                status_code,
            });
        }

        let updates_response = response.json::<TelegramGetUpdatesResponse>().await?;
        if !updates_response.ok {
            return Err(TelegramApiError::ApiReported(
                updates_response
                    .description
                    .unwrap_or_else(|| String::from("getUpdates returned ok=false")),
            ));
        }

        Ok(updates_response.result)
    }

    pub fn new(
        api_base_url: String,
        bot_token: String,
        request_timeout_seconds: u64,
    ) -> Result<Self, reqwest::Error> {
        let request_timeout = Duration::from_secs(request_timeout_seconds);
        let http_client = Client::builder().timeout(request_timeout).build()?;
        Ok(Self {
            api_base_url,
            bot_token,
            http_client,
        })
    }

    pub async fn send_message(
        &self,
        chat_identifier: i64,
        text: &str,
    ) -> Result<(), TelegramApiError> {
        let request_url = format!("{}/bot{}/sendMessage", self.api_base_url, self.bot_token);
        let response = self
            .http_client
            .post(request_url)
            .json(&TelegramSendMessageRequest {
                chat_id: chat_identifier,
                text: text.to_owned(),
            })
            .send()
            .await?;

        if !response.status().is_success() {
            let status_code = response.status();
            let response_body = response
                .text()
                .await
                .unwrap_or_else(|_| String::from("<unreadable response body>"));
            return Err(TelegramApiError::HttpStatus {
                response_body,
                status_code,
            });
        }

        let send_message_response = response.json::<TelegramSendMessageResponse>().await?;
        if !send_message_response.ok {
            return Err(TelegramApiError::ApiReported(
                send_message_response
                    .description
                    .unwrap_or_else(|| String::from("sendMessage returned ok=false")),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        Json, Router,
        extract::{Path, State},
        routing::{get, post},
    };
    use reqwest::StatusCode;
    use serde_json::{Value, json};
    use tokio::{net::TcpListener, sync::Mutex};

    use super::{TelegramApiClient, TelegramApiError};
    use crate::telegram::model::{TelegramIncomingMessage, TelegramUpdate};

    #[derive(Clone, Default)]
    struct MockTelegramState {
        sent_messages: Arc<Mutex<Vec<String>>>,
    }

    #[tokio::test]
    async fn get_updates_and_send_message_work_with_mock_server() {
        let mock_state = MockTelegramState::default();
        let mock_application = Router::new()
            .route(
                "/bot{token}/getUpdates",
                get(async |Path(_token): Path<String>| {
                    Json(json!({
                        "ok": true,
                        "result": [
                            {
                                "update_id": 10i64,
                                "message": {
                                    "chat": { "id": 77i64 },
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
                        if let Some(text) = payload.get("text").and_then(Value::as_str) {
                            route_state.sent_messages.lock().await.push(text.to_owned());
                        }
                        Json(json!({ "ok": true, "result": {} }))
                    },
                ),
            )
            .with_state(mock_state.clone());

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("9f1e2a7c");
        let bound_address = listener.local_addr().expect("1d73cb84");
        let server_task = tokio::spawn(async move {
            drop(axum::serve(listener, mock_application).await);
        });

        let api_client = TelegramApiClient::new(
            format!("http://{bound_address}"),
            String::from("test-token"),
            5,
        )
        .expect("b9d5f834");

        let updates = api_client.get_updates(0, 30).await.expect("d41a6fbe");
        assert_eq!(updates.len(), 1);

        let first_update: &TelegramUpdate = updates.first().expect("f6a8d2c4");
        let first_message: &TelegramIncomingMessage =
            first_update.message.as_ref().expect("7b3ed0aa");
        assert_eq!(first_message.text.as_deref(), Some("/health"));

        api_client
            .send_message(77, "hello from test")
            .await
            .expect("c8f1ab23");

        let captured_messages = mock_state.sent_messages.lock().await;
        assert_eq!(captured_messages.first().map(String::as_str), Some("hello from test"));
        drop(captured_messages);

        server_task.abort();
    }

    #[tokio::test]
    async fn get_updates_returns_api_reported_error_when_ok_false() {
        let mock_application = Router::new().route(
            "/bot{token}/getUpdates",
            get(async |Path(_token): Path<String>| {
                Json(json!({
                    "ok": false,
                    "description": "invalid token",
                    "result": []
                }))
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("d3a8f1b2");
        let bound_address = listener.local_addr().expect("c1e7a4d9");
        let server_task = tokio::spawn(async move {
            drop(axum::serve(listener, mock_application).await);
        });

        let api_client = TelegramApiClient::new(
            format!("http://{bound_address}"),
            String::from("test-token"),
            5,
        )
        .expect("e5b3c19a");

        let updates_result = api_client.get_updates(0, 30).await;
        assert!(matches!(
            updates_result,
            Err(TelegramApiError::ApiReported(description)) if description == "invalid token"
        ));

        server_task.abort();
    }

    #[tokio::test]
    async fn get_updates_returns_http_status_error() {
        let mock_application = Router::new().route(
            "/bot{token}/getUpdates",
            get(async |Path(_token): Path<String>| {
                (StatusCode::INTERNAL_SERVER_ERROR, "temporary failure")
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a7e4d29c");
        let bound_address = listener.local_addr().expect("b9f3a2d8");
        let server_task = tokio::spawn(async move {
            drop(axum::serve(listener, mock_application).await);
        });

        let api_client = TelegramApiClient::new(
            format!("http://{bound_address}"),
            String::from("test-token"),
            5,
        )
        .expect("f4c2d7a1");

        let updates_result = api_client.get_updates(0, 30).await;
        assert!(matches!(updates_result, Err(TelegramApiError::HttpStatus { .. })));

        server_task.abort();
    }

    #[tokio::test]
    async fn send_message_returns_api_reported_error_when_ok_false() {
        let mock_application = Router::new().route(
            "/bot{token}/sendMessage",
            post(async |Path(_token): Path<String>| {
                Json(json!({
                    "ok": false,
                    "description": "chat not found"
                }))
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("d8c1a4f7");
        let bound_address = listener.local_addr().expect("f9b2c6d3");
        let server_task = tokio::spawn(async move {
            drop(axum::serve(listener, mock_application).await);
        });

        let api_client = TelegramApiClient::new(
            format!("http://{bound_address}"),
            String::from("test-token"),
            5,
        )
        .expect("c6e2a9b4");

        let send_result = api_client.send_message(77, "hello").await;
        assert!(matches!(
            send_result,
            Err(TelegramApiError::ApiReported(description)) if description == "chat not found"
        ));

        server_task.abort();
    }
}
