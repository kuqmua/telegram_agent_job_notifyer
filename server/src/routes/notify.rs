use std::fmt::Write as _;

use axum::{Json, extract::State};
use serde::Serialize;
use shared::JobPayload;

use crate::{AppErr, St};
#[derive(Serialize)]
struct TelegramMessage {
    chat_id: i64,
    text: String,
}
#[allow(
    clippy::single_call_fn,
    reason = "Handler is wired exactly once in router setup by design"
)]
pub(crate) async fn handle(
    State(state): State<St>,
    Json(payload): Json<JobPayload>,
) -> Result<(), AppErr> {
    tracing::info!(
        "route=/notify message=notify_requested agent_name={} status={}",
        payload.agent_name,
        payload.status
    );
    let mut message = String::new();
    if let Some(result_text) = &payload.result {
        let _ = write!(message, "{result_text}");
    } else if let Some(error_text) = &payload.error {
        let _ = write!(message, "{error_text}");
    } else {
        let _ = write!(message, "(no result)");
    }
    let registered_chat_id = { *state.chat_id.lock().await };
    let chat_id = registered_chat_id.ok_or(AppErr::NoRegChat)?;
    let url = format!("https://api.telegram.org/bot{}/sendMessage", state.token);
    let telegram_message = TelegramMessage {
        chat_id,
        text: message,
    };
    let _response = state
        .client
        .post(&url)
        .json(&telegram_message)
        .send()
        .await?;
    tracing::info!("route=/notify message=notify_sent chat_id={chat_id}");
    Ok(())
}
