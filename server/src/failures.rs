use std::io::Error as InputOutputError;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use thiserror::Error;

use crate::{settings::EnvironmentError, telegram::api::TelegramApiError};
#[derive(Debug, Error)]
pub enum ServiceFailure {
    #[error("background task error: {0}")]
    BackgroundTask(String),
    #[error("configuration error: {0}")]
    Configuration(#[from] EnvironmentError),
    #[error("failed to build http client: {0}")]
    HttpClientBuild(#[from] reqwest::Error),
    #[error("io error: {0}")]
    InputOutput(#[from] InputOutputError),
    #[error("telegram api error: {0}")]
    TelegramApi(#[from] TelegramApiError),
}
impl IntoResponse for ServiceFailure {
    fn into_response(self) -> Response {
        let status_code = match self {
            Self::BackgroundTask(_)
            | Self::Configuration(_)
            | Self::HttpClientBuild(_)
            | Self::InputOutput(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::TelegramApi(_) => StatusCode::BAD_GATEWAY,
        };
        (status_code, self.to_string()).into_response()
    }
}
