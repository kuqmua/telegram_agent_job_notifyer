use std::io::Error as InputOutputError;

use thiserror::Error;

use crate::{
    settings::EnvironmentError,
    telegram::application_programming_interface::TelegramApplicationProgrammingInterfaceError,
};

#[derive(Debug, Error)]
pub enum ServiceFailure {
    #[error("configuration error: {0}")]
    Configuration(#[from] EnvironmentError),
    #[error("failed to build http client: {0}")]
    HyperTextTransferProtocolClientBuild(#[from] reqwest::Error),
    #[error("io error: {0}")]
    InputOutput(#[from] InputOutputError),
    #[error("telegram api error: {0}")]
    TelegramApplicationProgrammingInterface(#[from] TelegramApplicationProgrammingInterfaceError),
}
