use axum::{Json, extract::State};
use serde_json::json;

use crate::St;
#[allow(
    clippy::single_call_fn,
    reason = "Handler is wired exactly once in router setup by design"
)]
pub(crate) async fn handle(State(state): State<St>, Json(body): Json<serde_json::Value>) -> String {
    tracing::info!("route=/webhook/telegram message=webhook_received");
    if let Some(message) = body.get("message") {
        if let Some(sender) = message.get("from") {
            if let Some(chat_id) = sender.get("id").and_then(serde_json::Value::as_i64) {
                *state.chat_id.lock().await = Some(chat_id);
                let url = format!("https://api.telegram.org/bot{}/sendMessage", state.token);
                let payload = json!({ "chat_id": chat_id, "text": "Chat registered" });
                drop(state.client.post(&url).json(&payload).send().await);
                tracing::info!("route=/webhook/telegram message=chat_registered chat_id={chat_id}");
                return String::from("OK");
            }
        }
    }
    tracing::info!("route=/webhook/telegram message=ignored_payload");
    String::from("OK")
}
