use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::{AppErr, St};
#[derive(Serialize)]
struct TelegramMessage {
    chat_id: i64,
    text: String,
}
#[derive(Deserialize)]
struct TelegramApiResponse {
    description: Option<String>,
    ok: bool,
}
#[allow(
    clippy::single_call_fn,
    reason = "Handler is wired exactly once in router setup by design"
)]
pub(crate) async fn handle(State(state): State<St>) -> Result<&'static str, AppErr> {
    tracing::info!("route=/health msg=healthcheck");
    let registered_chat_id = { *state.chat_id.lock().await };
    let chat_id = registered_chat_id.ok_or(AppErr::NoRegChat)?;
    let url = format!("https://api.telegram.org/bot{}/sendMessage", state.token);
    let telegram_message = TelegramMessage {
        chat_id,
        text: String::from("Health check: bot is alive"),
    };
    let response = state
        .client
        .post(&url)
        .json(&telegram_message)
        .send()
        .await?
        .error_for_status()?;
    let telegram_response = response.json::<TelegramApiResponse>().await?;
    if !telegram_response.ok {
        return Err(AppErr::TgApi(
            telegram_response
                .description
                .unwrap_or_else(|| String::from("unknown")),
        ));
    }
    tracing::info!("route=/health msg=health_sent chat_id={chat_id}");
    Ok("OK")
}
