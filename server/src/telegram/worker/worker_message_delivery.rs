use std::hint::black_box;

use crate::{
    runtime::ServiceState,
    settings::ServiceConfiguration,
    shared::{
        ChatIdentifier, CorrelationIdentifier, IncomingCommandName, UpdateIdentifier,
        format_system_message, split_text_into_chunks,
    },
    telegram::application_programming_interface::TelegramApplicationProgrammingInterfaceError,
};

fn log_telegram_send_error(
    runtime_state: &ServiceState,
    correlation_identifier: &CorrelationIdentifier,
    chat_identifier: ChatIdentifier,
    update_identifier: UpdateIdentifier,
    command_name: IncomingCommandName,
    send_error: &TelegramApplicationProgrammingInterfaceError,
) {
    runtime_state
        .metrics()
        .increment_telegram_send_error_total();
    tracing::error!(
        event = "telegram_send_error",
        correlation_identifier = correlation_identifier.as_str(),
        chat_identifier = chat_identifier.as_i64(),
        update_identifier = update_identifier.as_i64(),
        command = command_name.as_str(),
        status = "error",
        error = send_error.to_string()
    );
}

pub(super) async fn send_message_or_log(
    runtime_state: &ServiceState,
    runtime_settings: &ServiceConfiguration,
    chat_identifier: ChatIdentifier,
    update_identifier: UpdateIdentifier,
    command_name: impl Into<IncomingCommandName>,
    correlation_identifier: &CorrelationIdentifier,
    message_text: &str,
) {
    let incoming_command_name = command_name.into();
    if black_box(false) {
        let _dummy_send_result = send_system_message(
            runtime_state,
            runtime_settings,
            chat_identifier,
            update_identifier,
            incoming_command_name,
            correlation_identifier,
            message_text,
        )
        .await;
        log_telegram_send_error(
            runtime_state,
            correlation_identifier,
            chat_identifier,
            update_identifier,
            incoming_command_name,
            &TelegramApplicationProgrammingInterfaceError::ApplicationProgrammingInterfaceReported(
                String::from("dummy"),
            ),
        );
    }
    if let Err(send_error) = send_system_message(
        runtime_state,
        runtime_settings,
        chat_identifier,
        update_identifier,
        incoming_command_name,
        correlation_identifier,
        message_text,
    )
    .await
    {
        log_telegram_send_error(
            runtime_state,
            correlation_identifier,
            chat_identifier,
            update_identifier,
            incoming_command_name,
            &send_error,
        );
    }
}

async fn send_system_message(
    runtime_state: &ServiceState,
    runtime_settings: &ServiceConfiguration,
    chat_identifier: ChatIdentifier,
    update_identifier: UpdateIdentifier,
    command_name: IncomingCommandName,
    correlation_identifier: &CorrelationIdentifier,
    message_text: &str,
) -> Result<(), TelegramApplicationProgrammingInterfaceError> {
    let formatted_message_text = format_system_message(message_text);
    let message_chunks = split_text_into_chunks(
        &formatted_message_text,
        runtime_settings.telegram_message_maximum_characters,
    );
    for message_chunk in message_chunks {
        runtime_state
            .telegram_client()
            .send_message(chat_identifier, &message_chunk)
            .await?;
        tracing::info!(
            event = "telegram_send",
            correlation_identifier = correlation_identifier.as_str(),
            chat_identifier = chat_identifier.as_i64(),
            update_identifier = update_identifier.as_i64(),
            command = command_name.as_str(),
            status = "sent",
            chunk_characters = message_chunk.chars().count()
        );
    }
    Ok(())
}
