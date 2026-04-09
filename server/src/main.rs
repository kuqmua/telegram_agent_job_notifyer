use std::{env, error::Error, fmt::Write as _, sync::Arc};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use tokio::{net::TcpListener, sync::Mutex};
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
}
impl IntoResponse for AppErr {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::MissingEnv(_) | Self::InvalidEnv(_) | Self::Rw(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
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
struct JobPayload {
    #[serde(rename = "elapsed_ms")]
    _elapsed_ms: Option<u64>,
    agent_name: String,
    error: Option<String>,
    result: Option<String>,
    status: String,
}
#[derive(Serialize)]
struct TgMsg {
    chat_id: i64,
    text: String,
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
    let st_clone = st.clone();
    let app = Router::new()
        .route(
            "/health",
            get(async || {
                tracing::info!("route=/health msg=healthcheck");
                "OK"
            }),
        )
        .route(
            "/notify",
            post(async |State(state): State<St>, Json(payload): Json<JobPayload>| {
                tracing::info!(
                    "route=/notify msg=notify_requested agent_name={} status={}",
                    payload.agent_name,
                    payload.status
                );
                let mut msg = String::new();
                if let Some(res) = &payload.result {
                    let _ = write!(msg, "{res}");
                } else if let Some(err) = &payload.error {
                    let _ = write!(msg, "{err}");
                } else {
                    let _ = write!(msg, "(no result)");
                }
                let chat_id = { *state.chat_id.lock().await };
                let cid = chat_id.ok_or(AppErr::NoRegChat)?;
                let url = format!("https://api.telegram.org/bot{}/sendMessage", state.token);
                let tg_payload = TgMsg {
                    chat_id: cid,
                    text: msg,
                };
                let _resp = state.client.post(&url).json(&tg_payload).send().await?;
                tracing::info!("route=/notify msg=notify_sent chat_id={cid}");
                Ok::<(), AppErr>(())
            }),
        )
        .route(
            "/webhook/telegram",
            post(async |State(state): State<St>, Json(body): Json<serde_json::Value>| -> String {
                tracing::info!("route=/webhook/telegram msg=webhook_received");
                if let Some(msg) = body.get("message") {
                    if let Some(from) = msg.get("from") {
                        if let Some(cid) = from.get("id").and_then(serde_json::Value::as_i64) {
                            *state.chat_id.lock().await = Some(cid);
                            let url =
                                format!("https://api.telegram.org/bot{}/sendMessage", state.token);
                            let payload = json!({ "chat_id": cid, "text": "Chat registered" });
                            drop(state.client.post(&url).json(&payload).send().await);
                            tracing::info!(
                                "route=/webhook/telegram msg=chat_registered chat_id={cid}"
                            );
                            return String::from("OK");
                        }
                    }
                }
                tracing::info!("route=/webhook/telegram msg=ignored_payload");
                String::from("OK")
            }),
        )
        .with_state(st_clone);
    let addr = format!("{host}:{port}");
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("Listening on {}", addr);
    if let Some(cid) = init_chat_id {
        tracing::info!("msg=chat_id_loaded_from_env chat_id={cid}");
    } else {
        tracing::info!("msg=chat_id_missing use_webhook_or_set_TELEGRAM_CHAT_ID");
    }
    axum::serve(listener, app).await?;
    Ok(())
}
