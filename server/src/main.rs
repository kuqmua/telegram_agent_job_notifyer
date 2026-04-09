mod routes;
use std::{env, error::Error, sync::Arc, time::Duration};

use axum::{
    Router,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use codex_cli::exec_prompt_capture;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{net::TcpListener, sync::Mutex, task::spawn_blocking, time::sleep};
use tracing as _;
#[derive(Error, Debug)]
enum AppErr {
    #[error("Invalid env var: {0}")]
    InvalidEnv(String),
    #[error("Missing env var: {0}")]
    MissingEnv(String),
    #[error("No registered chat")]
    NoRegChat,
    #[error("Reqwest error: {0}")]
    Rw(#[from] reqwest::Error),
    #[error("Telegram API error: {0}")]
    TgApi(String),
}
impl IntoResponse for AppErr {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::MissingEnv(_) | Self::InvalidEnv(_) | Self::Rw(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            Self::TgApi(_) => StatusCode::BAD_GATEWAY,
            Self::NoRegChat => StatusCode::SERVICE_UNAVAILABLE,
        };
        (status, self.to_string()).into_response()
    }
}
#[derive(Clone)]
struct St {
    chat_id: Arc<Mutex<Option<i64>>>,
    client: Client,
    token: String,
}
#[derive(Deserialize)]
struct TelegramGetUpdatesResponse {
    ok: bool,
    result: Vec<TelegramUpdate>,
}
#[derive(Deserialize)]
struct TelegramUpdate {
    message: Option<TelegramIncomingMessage>,
    update_id: i64,
}
#[derive(Deserialize)]
struct TelegramIncomingMessage {
    chat: TelegramChat,
    text: Option<String>,
}
#[derive(Deserialize)]
struct TelegramChat {
    id: i64,
}
#[derive(Serialize)]
struct TelegramSendMessage {
    chat_id: i64,
    text: String,
}
async fn send_telegram_msg(state: &St, chat_id: i64, text: String) -> Result<(), AppErr> {
    let send_message_url = format!("https://api.telegram.org/bot{}/sendMessage", state.token);
    let _response = state
        .client
        .post(send_message_url)
        .json(&TelegramSendMessage { chat_id, text })
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}
#[allow(
    clippy::infinite_loop,
    clippy::single_call_fn,
    reason = "Background polling runs for server lifetime and is spawned once from main"
)]
async fn run_telegram_polling(state: St) {
    let poll_retry_delay_seconds = env::var("TELEGRAM_POLL_RETRY_DELAY_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(2);
    let poll_timeout_seconds = env::var("TELEGRAM_POLL_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(30);
    tracing::info!("message=telegram_polling_mode_enabled");
    let mut update_offset = env::var("TELEGRAM_POLL_INITIAL_OFFSET")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0);
    loop {
        let poll_result = async {
            let get_updates_url = format!("https://api.telegram.org/bot{}/getUpdates", state.token);
            let poll_response = state
                .client
                .get(get_updates_url)
                .query(&[
                    ("offset", update_offset.to_string()),
                    ("timeout", poll_timeout_seconds.to_string()),
                ])
                .send()
                .await?
                .error_for_status()?
                .json::<TelegramGetUpdatesResponse>()
                .await?;
            if poll_response.ok {
                Ok(poll_response.result)
            } else {
                Err(AppErr::TgApi(String::from("getUpdates returned ok=false")))
            }
        }
        .await;
        match poll_result {
            Ok(updates) => {
                for update in updates {
                    update_offset = update.update_id.saturating_add(1);
                    let Some(message) = update.message else {
                        continue;
                    };
                    let chat_id = message.chat.id;
                    *state.chat_id.lock().await = Some(chat_id);
                    let text = message.text.unwrap_or_default();
                    tracing::info!(
                        "message=telegram_polling_update chat_id={} text={}",
                        chat_id,
                        text
                    );
                    if text == "/health" {
                        if let Err(error) = send_telegram_msg(
                            &state,
                            chat_id,
                            String::from("Health check: bot is alive"),
                        )
                        .await
                        {
                            tracing::error!(
                                "message=telegram_polling_health_send_error error={error}"
                            );
                        }
                        continue;
                    }
                    let Some(raw_prompt) = text.strip_prefix("/codex") else {
                        continue;
                    };
                    let prompt = raw_prompt.trim();
                    if prompt.is_empty() {
                        if let Err(error) = send_telegram_msg(
                            &state,
                            chat_id,
                            String::from("Usage: /codex <prompt>"),
                        )
                        .await
                        {
                            tracing::error!(
                                "message=telegram_polling_usage_send_error error={error}"
                            );
                        }
                        continue;
                    }
                    if let Err(error) = send_telegram_msg(
                        &state,
                        chat_id,
                        format!("Received message: {prompt}\nWork started"),
                    )
                    .await
                    {
                        tracing::error!(
                            "message=telegram_polling_started_send_error error={error}"
                        );
                    }
                    let prompt_owned = prompt.to_owned();
                    let exec_result =
                        spawn_blocking(move || exec_prompt_capture(&prompt_owned)).await;
                    let output_text = match exec_result {
                        Ok(Ok(output)) => {
                            let normalized_output = if output.trim().is_empty() {
                                String::from("(empty codex output)")
                            } else {
                                output
                            };
                            let max_length = 3500usize;
                            if normalized_output.chars().count() > max_length {
                                let prefix = normalized_output
                                    .chars()
                                    .take(max_length)
                                    .collect::<String>();
                                format!("{prefix}\n...[truncated]")
                            } else {
                                normalized_output
                            }
                        }
                        Ok(Err(error)) => format!("codex error: {error}"),
                        Err(join_error) => format!("codex task error: {join_error}"),
                    };
                    if let Err(error) =
                        send_telegram_msg(&state, chat_id, format!("Work finished\n{output_text}"))
                            .await
                    {
                        tracing::error!(
                            "message=telegram_polling_finished_send_error error={error}"
                        );
                    }
                }
            }
            Err(error) => {
                tracing::warn!("message=telegram_polling_error error={error}");
                sleep(Duration::from_secs(poll_retry_delay_seconds)).await;
            }
        }
    }
}
#[tokio::main]
#[allow(clippy::unwrap_in_result)]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    drop(dotenvy::dotenv());
    let token = env::var("TELEGRAM_BOT_TOKEN")
        .map_err(|_err| AppErr::MissingEnv("TELEGRAM_BOT_TOKEN".into()))?;
    let init_chat_id = env::var("TELEGRAM_CHAT_ID")
        .ok()
        .map(|value| {
            value
                .parse::<i64>()
                .map_err(|_err| AppErr::InvalidEnv("TELEGRAM_CHAT_ID parse error".into()))
        })
        .transpose()?;
    let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let port: u16 = env::var("PORT")
        .unwrap_or_else(|_| "8080".into())
        .parse()
        .map_err(|_err| AppErr::MissingEnv("PORT parse error".into()))?;
    let st = St {
        chat_id: Arc::new(Mutex::new(init_chat_id)),
        client: Client::new(),
        token,
    };
    drop(tracing_subscriber::fmt().try_init());
    let state_clone = st.clone();
    let app = Router::new()
        .route("/health", get(routes::health::handle))
        .with_state(state_clone);
    let addr = format!("{host}:{port}");
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("Listening on {}", addr);
    if let Some(cid) = init_chat_id {
        tracing::info!("msg=chat_id_loaded_from_env chat_id={cid}");
    } else {
        tracing::info!("msg=chat_id_missing set_TELEGRAM_CHAT_ID");
    }
    let _monitor_task = tokio::spawn(run_telegram_polling(st.clone()));
    axum::serve(listener, app).await?;
    Ok(())
}
