mod routes;
use std::{env, error::Error, sync::Arc, time::Duration};

use axum::{
    Router,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;
use tokio::{net::TcpListener, sync::Mutex, time::interval};
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
struct TelegramWebhookInfoResponse {
    ok: bool,
    result: TelegramWebhookInfo,
}
#[derive(Deserialize)]
struct TelegramWebhookInfo {
    last_error_message: Option<String>,
    pending_update_count: u64,
}
#[allow(
    clippy::infinite_loop,
    clippy::single_call_fn,
    reason = "Background monitor runs for server lifetime and is spawned once from main"
)]
async fn run_webhook_monitor(state: St) {
    let monitor_interval_seconds = env::var("WEBHOOK_MONITOR_INTERVAL_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(30);
    let pending_threshold = env::var("WEBHOOK_PENDING_THRESHOLD")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1);
    let mut previous_problem_message: Option<String> = None;
    let mut monitor_interval = interval(Duration::from_secs(monitor_interval_seconds));
    loop {
        let _tick = monitor_interval.tick().await;
        let webhook_info_result = async {
            let webhook_info_url =
                format!("https://api.telegram.org/bot{}/getWebhookInfo", state.token);
            let webhook_info_response = state
                .client
                .get(webhook_info_url)
                .send()
                .await?
                .error_for_status()?
                .json::<TelegramWebhookInfoResponse>()
                .await?;
            if webhook_info_response.ok {
                Ok(webhook_info_response.result)
            } else {
                Err(AppErr::TgApi(String::from("getWebhookInfo returned ok=false")))
            }
        }
        .await;
        let current_problem_message = match webhook_info_result {
            Ok(webhook_info) => {
                if let Some(last_error_message) = webhook_info.last_error_message {
                    Some(format!("Webhook error from Telegram: {last_error_message}"))
                } else if webhook_info.pending_update_count >= pending_threshold {
                    Some(format!(
                        "Webhook queue is growing: pending_update_count={}",
                        webhook_info.pending_update_count
                    ))
                } else {
                    None
                }
            }
            Err(error) => Some(format!("Webhook monitor failed: {error}")),
        };
        let has_state_changed = current_problem_message != previous_problem_message;
        if !has_state_changed {
            continue;
        }
        if let Some(problem_message) = &current_problem_message {
            tracing::warn!("message=webhook_monitor_problem details={problem_message}");
        } else {
            tracing::info!("message=webhook_monitor_recovered");
        }
        let registered_chat_id = { *state.chat_id.lock().await };
        if let Some(chat_id) = registered_chat_id {
            let message_for_chat = current_problem_message.as_ref().map_or_else(
                || String::from("Webhook recovered, updates are delivered again"),
                |problem_message| format!("Webhook issue detected\n{problem_message}"),
            );
            let send_message_result = state
                .client
                .post(format!("https://api.telegram.org/bot{}/sendMessage", state.token))
                .json(&serde_json::json!({ "chat_id": chat_id, "text": message_for_chat }))
                .send()
                .await
                .and_then(reqwest::Response::error_for_status);
            if let Err(error) = send_message_result {
                tracing::error!("message=webhook_monitor_send_error error={error}");
            }
        } else {
            tracing::warn!("message=webhook_monitor_no_chat_id");
        }
        previous_problem_message = current_problem_message;
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
        token: token.clone(),
    };
    drop(tracing_subscriber::fmt().try_init());
    let state_clone = st.clone();
    let app = Router::new()
        .route("/health", get(routes::health::handle))
        .route("/notify", post(routes::notify::handle))
        .route("/webhook/telegram/codex", post(routes::webhook_telegram_codex::handle))
        .route("/webhook/telegram", post(routes::webhook_telegram::handle))
        .with_state(state_clone);
    let addr = format!("{host}:{port}");
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("Listening on {}", addr);
    if let Some(cid) = init_chat_id {
        tracing::info!("msg=chat_id_loaded_from_env chat_id={cid}");
    } else {
        tracing::info!("msg=chat_id_missing set_TELEGRAM_CHAT_ID");
    }
    let _monitor_task = tokio::spawn(run_webhook_monitor(st.clone()));
    axum::serve(listener, app).await?;
    Ok(())
}
