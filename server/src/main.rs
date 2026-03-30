use std::{env, sync::Arc};

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
            Self::MissingEnv(_) | Self::Rw(_) => StatusCode::INTERNAL_SERVER_ERROR,
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
    agent_name: String,
    elapsed_ms: Option<u64>,
    error: Option<String>,
    result: Option<String>,
    status: String,
}
#[derive(Serialize)]
struct TgMsg {
    chat_id: i64,
    parse_mode: String,
    text: String,
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    drop(dotenvy::dotenv());
    let token = env::var("TELEGRAM_BOT_TOKEN")
        .map_err(|_err| AppErr::MissingEnv("TELEGRAM_BOT_TOKEN".into()))?;
    let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let port: u16 = env::var("PORT")
        .unwrap_or_else(|_| "8080".into())
        .parse()
        .map_err(|_err| AppErr::MissingEnv("PORT parse error".into()))?;
    let st = St {
        chat_id: Arc::new(Mutex::new(None)),
        client: Client::new(),
        token: token.clone(),
    };
    drop(tracing_subscriber::fmt().try_init());
    let st_clone = st.clone();
    let app = Router::new()
        .route("/health", get(async || "OK"))
        .route(
            "/notify",
            post(async |State(state): State<St>, Json(payload): Json<JobPayload>| {
                let mut msg = format!(
                    "<b>{}</b>\nAgent: {}\nStatus: {}",
                    payload.status.to_uppercase(),
                    payload.agent_name,
                    payload.status
                );
                if let Some(res) = &payload.result {
                    msg.push_str(&format!("\nResult: {}", res));
                }
                if let Some(err) = &payload.error {
                    msg.push_str(&format!("\nError: {}", err));
                }
                if let Some(time) = payload.elapsed_ms {
                    msg.push_str(&format!("\nTime: {}ms", time));
                }
                let chat_id = { *state.chat_id.lock().await };
                let cid = chat_id.ok_or(AppErr::NoRegChat)?;
                let url = format!("https://api.telegram.org/bot{}/sendMessage", state.token);
                let tg_payload = TgMsg {
                    chat_id: cid,
                    parse_mode: "HTML".into(),
                    text: msg,
                };
                let _resp = state.client.post(&url).json(&tg_payload).send().await?;
                Ok::<(), AppErr>(())
            }),
        )
        .route(
            "/webhook/telegram",
            post(async |State(state): State<St>, Json(body): Json<serde_json::Value>| -> String {
                if let Some(msg) = body.get("message") {
                    if let Some(from) = msg.get("from") {
                        if let Some(cid) = from.get("id").and_then(|val| val.as_i64()) {
                            *state.chat_id.lock().await = Some(cid);
                            let url =
                                format!("https://api.telegram.org/bot{}/sendMessage", state.token);
                            let payload = json!({ "chat_id": cid, "text": "Chat registered" });
                            drop(state.client.post(&url).json(&payload).send().await);
                            return String::from("OK");
                        }
                    }
                }
                String::from("OK")
            }),
        )
        .with_state(st_clone);
    let addr = format!("{host}:{port}");
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("Listening on {}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}
