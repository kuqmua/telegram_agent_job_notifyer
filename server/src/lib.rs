pub mod failures;
pub mod runtime;
pub mod settings;
pub mod telegram;

use std::{io::Error as InputOutputError, sync::Arc};

use axum as _;
use dotenvy as _;
use serde_json as _;
pub use telegram_agent_shared as shared;
use tokio::{signal::ctrl_c, sync::watch};
use tracing_subscriber as _;

use crate::{
    failures::ServiceFailure,
    runtime::ServiceState,
    settings::ServiceConfiguration,
    telegram::{
        application_programming_interface::TelegramApplicationProgrammingInterfaceClient,
        worker::run_updates_loop,
    },
};

pub async fn run_service(
    service_configuration: ServiceConfiguration,
) -> Result<(), ServiceFailure> {
    let telegram_application_programming_interface_client =
        TelegramApplicationProgrammingInterfaceClient::new(
            service_configuration
                .telegram_application_programming_interface_base_uniform_resource_locator
                .clone(),
            service_configuration.telegram_bot_token.clone(),
            service_configuration.telegram_hyper_text_transfer_protocol_timeout_seconds,
        )?;
    let service_state = ServiceState::new(
        telegram_application_programming_interface_client,
        &service_configuration,
    );
    let worker_service_state = service_state.clone();
    let worker_service_configuration = Arc::new(service_configuration);
    let (shutdown_sender, shutdown_receiver) = watch::channel(false);
    let worker_shutdown_receiver = shutdown_receiver.clone();
    let worker_task = tokio::spawn(async move {
        run_updates_loop(
            worker_service_state,
            worker_service_configuration,
            worker_shutdown_receiver,
        )
        .await;
    });
    ctrl_c().await?;
    let _send_result = shutdown_sender.send(true);
    let worker_join_result = worker_task.await;
    if let Err(join_error) = worker_join_result {
        return Err(ServiceFailure::InputOutput(InputOutputError::other(format!(
            "worker task failed: {join_error}"
        ))));
    }
    Ok(())
}
