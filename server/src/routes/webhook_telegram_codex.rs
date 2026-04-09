use axum::{Json, extract::State};
use codex_cli::exec_prompt_capture;
use serde::Serialize;
use serde_json::Value;
use tokio::task::spawn_blocking;

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
    tracing::info!("route=/webhook/telegram/codex message=webhook_received");
    let Some(message) = body.get("message") else {
        tracing::info!("route=/webhook/telegram/codex message=ignored_payload");
        return String::from("OK");
    };
    let parsed_chat_id = message
        .get("chat")
        .and_then(|chat_value| chat_value.get("id"))
        .and_then(Value::as_i64)
        .or_else(|| {
            message
                .get("from")
                .and_then(|sender_value| sender_value.get("id"))
                .and_then(Value::as_i64)
        });
    let Some(chat_id) = parsed_chat_id else {
        tracing::info!("route=/webhook/telegram/codex message=no_chat_id");
        return String::from("OK");
    };
    *state.chat_id.lock().await = Some(chat_id);
    let text = message.get("text").and_then(Value::as_str).unwrap_or("");
    let Some(raw_prompt) = text.strip_prefix("/codex") else {
        tracing::info!("route=/webhook/telegram/codex message=not_codex_command");
        return String::from("OK");
    };
    let prompt = raw_prompt.trim();
    let response_text = if prompt.is_empty() {
        String::from("Usage: /codex <prompt>")
    } else {
        let prompt_owned = prompt.to_owned();
        let run_result = spawn_blocking(move || exec_prompt_capture(&prompt_owned)).await;
        match run_result {
            Ok(Ok(output)) => {
                let normalized_output = if output.trim().is_empty() {
                    String::from("(empty codex output)")
                } else {
                    output
                };
                let max_len = 3500usize;
                if normalized_output.chars().count() > max_len {
                    let prefix = normalized_output.chars().take(max_len).collect::<String>();
                    format!("{prefix}\n...[truncated]")
                } else {
                    normalized_output
                }
            }
            Ok(Err(error)) => format!("codex error: {error}"),
            Err(join_error) => format!("codex task error: {join_error}"),
        }
    };
    let url = format!("https://api.telegram.org/bot{}/sendMessage", state.token);
    let telegram_message = TelegramMessage {
        chat_id,
        text: response_text,
    };
    let send_result = state.client.post(&url).json(&telegram_message).send().await;
    if let Err(error) = send_result {
        tracing::error!("route=/webhook/telegram/codex message=send_error error={error}");
    } else {
        tracing::info!("route=/webhook/telegram/codex message=sent chat_id={chat_id}");
    }
    String::from("OK")
}
