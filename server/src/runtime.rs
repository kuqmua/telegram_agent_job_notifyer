use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::{
    settings::ServiceConfiguration,
    shared::{ChatIdentifier, SenderUsername},
    telegram::application_programming_interface::TelegramApplicationProgrammingInterfaceClient,
};

#[derive(Clone, Debug)]
pub struct ServiceState {
    polling_ready: Arc<AtomicBool>,
    telegram_allowed_username: Option<SenderUsername>,
    telegram_chat_identifier: Option<ChatIdentifier>,
    telegram_client: TelegramApplicationProgrammingInterfaceClient,
}

impl ServiceState {
    #[must_use]
    pub fn is_polling_ready(&self) -> bool {
        self.polling_ready.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn is_update_authorized(
        &self,
        incoming_chat_identifier: ChatIdentifier,
        incoming_sender_username: Option<&SenderUsername>,
    ) -> bool {
        if self
            .telegram_chat_identifier
            .is_some_and(|configured_chat_identifier| {
                configured_chat_identifier != incoming_chat_identifier
            })
        {
            return false;
        }
        if let Some(configured_allowed_username) = &self.telegram_allowed_username {
            let Some(incoming_username) = incoming_sender_username else {
                return false;
            };
            return configured_allowed_username
                .as_str()
                .eq_ignore_ascii_case(incoming_username.as_str());
        }
        true
    }

    #[must_use]
    pub fn new(
        telegram_client: TelegramApplicationProgrammingInterfaceClient,
        service_configuration: &ServiceConfiguration,
    ) -> Self {
        Self {
            polling_ready: Arc::new(AtomicBool::new(false)),
            telegram_allowed_username: service_configuration.telegram_allowed_username.clone(),
            telegram_chat_identifier: service_configuration.telegram_chat_identifier,
            telegram_client,
        }
    }

    pub fn set_polling_ready(&self, value: bool) {
        self.polling_ready.store(value, Ordering::Relaxed);
    }

    #[must_use]
    pub const fn telegram_client(&self) -> &TelegramApplicationProgrammingInterfaceClient {
        &self.telegram_client
    }
}
