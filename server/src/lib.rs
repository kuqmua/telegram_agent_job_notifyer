pub mod failures;
pub mod routes;
pub mod runtime;
pub mod settings;
pub mod task_manager;
pub mod telegram;
use std::{
    env::{var, var_os},
    ffi::OsString,
    process::{Command, Stdio},
    sync::Arc,
};

use axum::{Router, routing::get};
use dotenvy as _;
use serde_json as _;
pub use telegram_agent_shared as shared;
use telegram_agent_shared as _;
use tokio::{net::TcpListener, signal::ctrl_c, sync::watch};
use tracing_subscriber as _;

use crate::{
    failures::ServiceFailure,
    runtime::ServiceState,
    settings::ServiceConfiguration,
    task_manager::TaskManager,
    telegram::{
        application_programming_interface::TelegramApplicationProgrammingInterfaceClient,
        worker::run_updates_loop,
    },
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
    let telegram_application_programming_interface_client =
        TelegramApplicationProgrammingInterfaceClient::new(
            runtime_settings
                .telegram_application_programming_interface_base_uniform_resource_locator
                .clone(),
            runtime_settings.telegram_bot_token.clone(),
            runtime_settings.telegram_hyper_text_transfer_protocol_timeout_seconds,
        )?;
    let task_manager = TaskManager::new(
        runtime_settings.task_history_file_path.clone(),
        runtime_settings.task_history_maximum_size,
        runtime_settings.prompt_maximum_characters,
        runtime_settings.task_rate_limit_per_minute,
    );
    Ok(ServiceState::new(
        telegram_application_programming_interface_client,
        runtime_settings.telegram_admin_usernames.clone(),
        runtime_settings.telegram_allowed_username.clone(),
        runtime_settings.telegram_chat_identifier,
        runtime_settings.codex_max_parallel_tasks,
        runtime_settings.update_processing_max_parallel_tasks,
        task_manager,
    ))
}
pub async fn run_service(runtime_settings: ServiceConfiguration) -> Result<(), ServiceFailure> {
    let codex_require_login_status = var("CODEX_REQUIRE_LOGIN_STATUS").map_or_else(
        |_| Ok(true),
        |variable_value| {
            variable_value.parse::<bool>().map_err(|parse_error| {
                ServiceFailure::StartupPreflight(format!(
                    "invalid CODEX_REQUIRE_LOGIN_STATUS: {parse_error}"
                ))
            })
        },
    )?;
    if codex_require_login_status {
        let codex_binary_path = if let Some(configured_codex_binary_path) =
            runtime_settings.codex_binary_path.as_deref()
        {
            OsString::from(configured_codex_binary_path)
        } else if let Some(codex_binary_path_from_environment) = var_os("CODEX_BIN") {
            codex_binary_path_from_environment
        } else {
            let candidate_binary_paths = ["codex", "codex-cli"];
            let mut discovered_binary_path: Option<OsString> = None;
            for candidate_binary_path in candidate_binary_paths {
                let probe_result = Command::new(candidate_binary_path)
                    .arg("--version")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
                if probe_result.is_ok_and(|status| status.success()) {
                    discovered_binary_path = Some(OsString::from(candidate_binary_path));
                    break;
                }
            }
            discovered_binary_path.ok_or_else(|| {
                ServiceFailure::StartupPreflight(String::from(
                    "codex binary not found for startup preflight; configure CODEX_BINARY_PATH",
                ))
            })?
        };
        let login_status_output = Command::new(&codex_binary_path)
            .args(["login", "status"])
            .stdin(Stdio::null())
            .output()
            .map_err(|io_error| {
                ServiceFailure::StartupPreflight(format!(
                    "failed to execute codex login status: {io_error}"
                ))
            })?;
        if !login_status_output.status.success() {
            let error_details = String::from_utf8_lossy(&login_status_output.stderr)
                .trim()
                .to_owned();
            let normalized_error_details = if error_details.is_empty() {
                String::from("no stderr output")
            } else {
                error_details
            };
            return Err(ServiceFailure::StartupPreflight(format!(
                "codex login status failed: {normalized_error_details}"
            )));
        }
    }
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
