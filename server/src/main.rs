use std::process::exit;

use axum as _;
use openai_command_runtime as _;
use reqwest as _;
use serde as _;
use serde_json as _;
use server::{run_service, settings::ServiceConfiguration};
use telegram_agent_shared as _;
use thiserror as _;
use tracing as _;
#[tokio::main]
async fn main() {
    drop(dotenvy::dotenv_override());
    drop(tracing_subscriber::fmt().try_init());
    let runtime_settings = match ServiceConfiguration::from_env() {
        Ok(parsed_configuration) => parsed_configuration,
        Err(configuration_error) => {
            tracing::error!("startup configuration error: {configuration_error}");
            exit(1);
        }
    };
    if let Err(service_error) = run_service(runtime_settings).await {
        tracing::error!("service error: {service_error}");
        exit(1);
    }
}
