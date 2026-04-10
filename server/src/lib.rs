pub mod failures;
pub mod routes;
pub mod runtime;
pub mod settings;
pub mod telegram;

use std::sync::Arc;

use axum::{Router, routing::get};
use dotenvy as _;
use serde_json as _;
use tokio::{net::TcpListener, signal::ctrl_c, sync::watch};
use tracing_subscriber as _;

use crate::{
    failures::ServiceFailure,
    runtime::ServiceState,
    settings::ServiceConfiguration,
    telegram::{api::TelegramApiClient, worker::run_updates_loop},
};

pub fn build_router(runtime_state: ServiceState) -> Router {
    Router::new()
        .route("/health", get(routes::status::health_probe))
        .route("/health/live", get(routes::status::live_probe))
        .route("/health/ready", get(routes::status::ready_probe))
        .route("/metrics", get(routes::status::metrics_probe))
        .with_state(runtime_state)
}

pub fn build_runtime_state(
    runtime_settings: &ServiceConfiguration,
) -> Result<ServiceState, ServiceFailure> {
    let telegram_api_client = TelegramApiClient::new(
        runtime_settings.telegram_api_base_url.clone(),
        runtime_settings.telegram_bot_token.clone(),
        runtime_settings.telegram_http_timeout_seconds,
    )?;

    Ok(ServiceState::new(
        telegram_api_client,
        runtime_settings.telegram_chat_identifier,
        runtime_settings.codex_max_parallel_tasks,
        runtime_settings.update_processing_max_parallel_tasks,
    ))
}

pub async fn run_service(runtime_settings: ServiceConfiguration) -> Result<(), ServiceFailure> {
    let runtime_state = build_runtime_state(&runtime_settings)?;
    let application_router = build_router(runtime_state.clone());

    let server_bind_address = format!("{}:{}", runtime_settings.host, runtime_settings.port);
    let server_listener = TcpListener::bind(&server_bind_address).await?;
    tracing::info!(event = "server_start", status = "ok", address = server_bind_address.as_str());

    let worker_runtime_state = runtime_state.clone();
    let worker_runtime_settings = Arc::new(runtime_settings);
    let (shutdown_sender, shutdown_receiver) = watch::channel(false);

    let worker_shutdown_receiver = shutdown_receiver.clone();
    let worker_task = tokio::spawn(async move {
        run_updates_loop(worker_runtime_state, worker_runtime_settings, worker_shutdown_receiver)
            .await;
    });

    let ctrl_c_shutdown_sender = shutdown_sender.clone();
    let ctrl_c_task = tokio::spawn(async move {
        if ctrl_c().await.is_ok() {
            tracing::info!(event = "shutdown_signal_received", status = "ok");
            let _send_result = ctrl_c_shutdown_sender.send(true);
        }
    });

    let server_shutdown_receiver = shutdown_receiver.clone();
    let server_future =
        axum::serve(server_listener, application_router).with_graceful_shutdown(async move {
            let mut graceful_shutdown_receiver = server_shutdown_receiver;
            while !*graceful_shutdown_receiver.borrow() {
                if graceful_shutdown_receiver.changed().await.is_err() {
                    break;
                }
            }
        });

    let server_result = server_future.await;
    let _send_result = shutdown_sender.send(true);

    worker_task.abort();
    let worker_join_result = worker_task.await;
    if let Err(join_error) = worker_join_result {
        if !join_error.is_cancelled() {
            return Err(ServiceFailure::BackgroundTask(join_error.to_string()));
        }
    }

    ctrl_c_task.abort();
    server_result?;

    Ok(())
}
