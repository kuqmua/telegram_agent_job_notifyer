mod routes;
use std::{env, error::Error, sync::Arc};

use axum::{
    Router,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use reqwest::Client;
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
    axum::serve(listener, app).await?;
    Ok(())
}
