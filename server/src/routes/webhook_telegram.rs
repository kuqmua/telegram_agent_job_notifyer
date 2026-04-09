use axum::{Json, extract::State};
use serde::Serialize;
use serde_json::Value;

use crate::St;
#[derive(Serialize)]
struct TelegramMessage {
    chat_id: i64,
    text: String,
}
#[allow(
    clippy::single_call_fn,
    reason = "Handler is wired exactly once in router setup by design"
)]
pub(crate) async fn handle(State(state): State<St>, Json(body): Json<Value>) -> String {
    tracing::info!("route=/webhook/telegram message=webhook_received");
    let Some(message) = body.get("message") else {
        tracing::info!("route=/webhook/telegram message=ignored_payload");
        return String::from("OK");
    };
    let sender = message.get("from");
    let chat = message.get("chat");
    let message_text = message.get("text").and_then(Value::as_str);
    let chat_id_from_chat = chat
        .and_then(|chat_value| chat_value.get("id"))
        .and_then(Value::as_i64);
    let chat_id_from_sender = sender
        .and_then(|sender_value| sender_value.get("id"))
        .and_then(Value::as_i64);
    let Some(chat_id) = chat_id_from_chat.or(chat_id_from_sender) else {
        tracing::info!("route=/webhook/telegram message=no_chat_id");
        return String::from("OK");
    };
    *state.chat_id.lock().await = Some(chat_id);
    tracing::info!("route=/webhook/telegram message=chat_registered chat_id={chat_id}");
    if message_text == Some("/health") {
        let url = format!("https://api.telegram.org/bot{}/sendMessage", state.token);
        let telegram_message = TelegramMessage {
            chat_id,
            text: String::from("Health check: bot is alive"),
        };
        let send_result = state.client.post(&url).json(&telegram_message).send().await;
        if let Err(error) = send_result {
            tracing::error!("route=/webhook/telegram message=health_send_error error={error}");
        } else {
            tracing::info!("route=/webhook/telegram message=health_sent chat_id={chat_id}");
        }
    }
    String::from("OK")
}
