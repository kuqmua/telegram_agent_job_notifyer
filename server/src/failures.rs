use std::io::Error as InputOutputStreamError;

use axum::{
    http::StatusCode as HyperTextTransferProtocolStatusCode,
    response::{IntoResponse, Response},
};
use thiserror::Error;

use crate::{
    settings::EnvironmentError,
    telegram::application_programming_interface::TelegramApplicationProgrammingInterfaceError,
};
#[derive(Debug, Error)]
pub enum ServiceFailure {
    #[error("background task error: {0}")]
    BackgroundTask(String),
    #[error("configuration error: {0}")]
    Configuration(#[from] EnvironmentError),
    #[error("failed to build http client: {0}")]
    HyperTextTransferProtocolClientBuild(#[from] reqwest::Error),
    #[error("io error: {0}")]
    InputOutputStream(#[from] InputOutputStreamError),
    #[error("startup preflight error: {0}")]
    StartupPreflight(String),
    #[error("telegram api error: {0}")]
    TelegramApplicationProgrammingInterface(#[from] TelegramApplicationProgrammingInterfaceError),
}
impl IntoResponse for ServiceFailure {
    fn into_response(self) -> Response {
        let status_code = match self {
            Self::BackgroundTask(_)
            | Self::Configuration(_)
            | Self::HyperTextTransferProtocolClientBuild(_)
            | Self::InputOutputStream(_)
            | Self::StartupPreflight(_) => {
                HyperTextTransferProtocolStatusCode::INTERNAL_SERVER_ERROR
            }
            Self::TelegramApplicationProgrammingInterface(_) => {
                HyperTextTransferProtocolStatusCode::BAD_GATEWAY
            }
        };
        (status_code, self.to_string()).into_response()
    }
}
