use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use tokio::sync::{AcquireError, OwnedSemaphorePermit, Semaphore, TryAcquireError};

use crate::{task_manager::TaskManager, telegram::api::TelegramApiClient};

#[derive(Debug)]
pub struct ServiceMetrics {
    codex_execution_duration_milliseconds_count: AtomicU64,
    codex_execution_duration_milliseconds_total: AtomicU64,
    codex_execution_error_total: AtomicU64,
    codex_execution_timeout_total: AtomicU64,
    polling_error_total: AtomicU64,
    polling_request_total: AtomicU64,
    polling_retry_total: AtomicU64,
    polling_success_total: AtomicU64,
    polling_total_duration_milliseconds: AtomicU64,
    task_cancelled_total: AtomicU64,
    task_completed_total: AtomicU64,
    task_created_total: AtomicU64,
    task_failed_total: AtomicU64,
    task_running_total: AtomicU64,
    task_timeout_total: AtomicU64,
    telegram_send_error_total: AtomicU64,
    update_duplicate_total: AtomicU64,
}

impl Default for ServiceMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl ServiceMetrics {
    pub fn decrement_task_running_total(&self) {
        let current_running_total = self.task_running_total.load(Ordering::Relaxed);
        if current_running_total == 0 {
            return;
        }
        let _previous_running_total = self.task_running_total.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn increment_codex_execution_error_total(&self) {
        let _previous_value = self
            .codex_execution_error_total
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_codex_execution_timeout_total(&self) {
        let _previous_value = self
            .codex_execution_timeout_total
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_polling_error_total(&self) {
        let _previous_value = self.polling_error_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_polling_request_total(&self) {
        let _previous_value = self.polling_request_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_polling_retry_total(&self) {
        let _previous_value = self.polling_retry_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_polling_success_total(&self) {
        let _previous_value = self.polling_success_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_task_cancelled_total(&self) {
        let _previous_value = self.task_cancelled_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_task_completed_total(&self) {
        let _previous_value = self.task_completed_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_task_created_total(&self) {
        let _previous_value = self.task_created_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_task_failed_total(&self) {
        let _previous_value = self.task_failed_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_task_running_total(&self) {
        let _previous_value = self.task_running_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_task_timeout_total(&self) {
        let _previous_value = self.task_timeout_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_telegram_send_error_total(&self) {
        let _previous_value = self
            .telegram_send_error_total
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_update_duplicate_total(&self) {
        let _previous_value = self.update_duplicate_total.fetch_add(1, Ordering::Relaxed);
    }

    #[must_use]
    pub const fn new() -> Self {
        Self {
            codex_execution_duration_milliseconds_count: AtomicU64::new(0),
            codex_execution_duration_milliseconds_total: AtomicU64::new(0),
            codex_execution_error_total: AtomicU64::new(0),
            codex_execution_timeout_total: AtomicU64::new(0),
            polling_error_total: AtomicU64::new(0),
            polling_request_total: AtomicU64::new(0),
            polling_retry_total: AtomicU64::new(0),
            polling_success_total: AtomicU64::new(0),
            polling_total_duration_milliseconds: AtomicU64::new(0),
            task_cancelled_total: AtomicU64::new(0),
            task_completed_total: AtomicU64::new(0),
            task_created_total: AtomicU64::new(0),
            task_failed_total: AtomicU64::new(0),
            task_running_total: AtomicU64::new(0),
            task_timeout_total: AtomicU64::new(0),
            telegram_send_error_total: AtomicU64::new(0),
            update_duplicate_total: AtomicU64::new(0),
        }
    }

    pub fn record_codex_execution_duration_milliseconds(&self, duration_milliseconds: u128) {
        let safe_duration_milliseconds = u64::try_from(duration_milliseconds).unwrap_or(u64::MAX);
        let _previous_total_duration = self
            .codex_execution_duration_milliseconds_total
            .fetch_add(safe_duration_milliseconds, Ordering::Relaxed);
        let _previous_count = self
            .codex_execution_duration_milliseconds_count
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_polling_duration_milliseconds(&self, duration_milliseconds: u128) {
        let safe_duration_milliseconds = u64::try_from(duration_milliseconds).unwrap_or(u64::MAX);
        let _previous_value = self
            .polling_total_duration_milliseconds
            .fetch_add(safe_duration_milliseconds, Ordering::Relaxed);
    }

    #[must_use]
    pub fn render_prometheus(
        &self,
        is_polling_ready: bool,
        configured_chat_identifier: Option<i64>,
    ) -> String {
        let polling_ready_value = u8::from(is_polling_ready);
        let configured_chat_identifier_value = i64::from(configured_chat_identifier.is_some());
        format!(
            "\
# HELP polling_ready Readiness of polling loop
# TYPE polling_ready gauge
polling_ready {polling_ready_value}
# HELP telegram_chat_identifier_configured Whether TELEGRAM_CHAT_ID is configured
# TYPE telegram_chat_identifier_configured gauge
telegram_chat_identifier_configured {configured_chat_identifier_value}
# HELP telegram_poll_requests_total Total number of getUpdates requests
# TYPE telegram_poll_requests_total counter
telegram_poll_requests_total {}
# HELP telegram_poll_success_total Total number of successful getUpdates responses
# TYPE telegram_poll_success_total counter
telegram_poll_success_total {}
# HELP telegram_poll_errors_total Total number of getUpdates errors
# TYPE telegram_poll_errors_total counter
telegram_poll_errors_total {}
# HELP telegram_poll_retries_total Total number of polling retries after temporary failures
# TYPE telegram_poll_retries_total counter
telegram_poll_retries_total {}
# HELP telegram_poll_duration_milliseconds_total Total duration of successful polling calls in \
             milliseconds
# TYPE telegram_poll_duration_milliseconds_total counter
telegram_poll_duration_milliseconds_total {}
# HELP telegram_send_errors_total Total number of sendMessage errors
# TYPE telegram_send_errors_total counter
telegram_send_errors_total {}
# HELP codex_execution_errors_total Total number of codex execution failures
# TYPE codex_execution_errors_total counter
codex_execution_errors_total {}
# HELP codex_execution_timeouts_total Total number of codex execution timeouts
# TYPE codex_execution_timeouts_total counter
codex_execution_timeouts_total {}
# HELP codex_execution_duration_milliseconds_total Total codex execution duration in milliseconds
# TYPE codex_execution_duration_milliseconds_total counter
codex_execution_duration_milliseconds_total {}
# HELP codex_execution_duration_milliseconds_count Total count of codex executions with measured \
             duration
# TYPE codex_execution_duration_milliseconds_count counter
codex_execution_duration_milliseconds_count {}
# HELP update_duplicates_total Total number of deduplicated updates
# TYPE update_duplicates_total counter
update_duplicates_total {}
# HELP task_created_total Total tasks created
# TYPE task_created_total counter
task_created_total {}
# HELP task_running_total Running tasks at this moment
# TYPE task_running_total gauge
task_running_total {}
# HELP task_completed_total Total tasks completed successfully
# TYPE task_completed_total counter
task_completed_total {}
# HELP task_failed_total Total tasks failed
# TYPE task_failed_total counter
task_failed_total {}
# HELP task_cancelled_total Total tasks cancelled
# TYPE task_cancelled_total counter
task_cancelled_total {}
# HELP task_timeout_total Total tasks timed out
# TYPE task_timeout_total counter
task_timeout_total {}
",
            self.polling_request_total.load(Ordering::Relaxed),
            self.polling_success_total.load(Ordering::Relaxed),
            self.polling_error_total.load(Ordering::Relaxed),
            self.polling_retry_total.load(Ordering::Relaxed),
            self.polling_total_duration_milliseconds
                .load(Ordering::Relaxed),
            self.telegram_send_error_total.load(Ordering::Relaxed),
            self.codex_execution_error_total.load(Ordering::Relaxed),
            self.codex_execution_timeout_total.load(Ordering::Relaxed),
            self.codex_execution_duration_milliseconds_total
                .load(Ordering::Relaxed),
            self.codex_execution_duration_milliseconds_count
                .load(Ordering::Relaxed),
            self.update_duplicate_total.load(Ordering::Relaxed),
            self.task_created_total.load(Ordering::Relaxed),
            self.task_running_total.load(Ordering::Relaxed),
            self.task_completed_total.load(Ordering::Relaxed),
            self.task_failed_total.load(Ordering::Relaxed),
            self.task_cancelled_total.load(Ordering::Relaxed),
            self.task_timeout_total.load(Ordering::Relaxed),
        )
    }
}

#[derive(Clone, Debug)]
pub struct ServiceState {
    codex_semaphore: Arc<Semaphore>,
    configured_telegram_admin_usernames: Vec<String>,
    configured_telegram_allowed_username: Option<String>,
    configured_telegram_chat_identifier: Option<i64>,
    correlation_identifier_counter: Arc<AtomicU64>,
    metrics: Arc<ServiceMetrics>,
    polling_is_ready: Arc<AtomicBool>,
    task_manager: TaskManager,
    telegram_api_client: TelegramApiClient,
    update_processing_semaphore: Arc<Semaphore>,
}

impl ServiceState {
    pub async fn acquire_codex_permit(&self) -> Result<OwnedSemaphorePermit, AcquireError> {
        Arc::clone(&self.codex_semaphore).acquire_owned().await
    }

    #[must_use]
    pub const fn configured_telegram_chat_identifier(&self) -> Option<i64> {
        self.configured_telegram_chat_identifier
    }

    #[must_use]
    pub fn is_chat_authorized(&self, chat_identifier: i64) -> bool {
        self.configured_telegram_chat_identifier
            .is_none_or(|configured_chat_identifier| configured_chat_identifier == chat_identifier)
    }

    #[must_use]
    pub fn is_polling_ready(&self) -> bool {
        self.polling_is_ready.load(Ordering::SeqCst)
    }

    #[must_use]
    pub fn is_sender_admin(&self, sender_username: Option<&str>) -> bool {
        let Some(incoming_sender_username) = sender_username else {
            return false;
        };
        self.configured_telegram_admin_usernames
            .iter()
            .any(|admin_username| incoming_sender_username.eq_ignore_ascii_case(admin_username))
    }

    #[must_use]
    pub fn is_sender_authorized(&self, sender_username: Option<&str>) -> bool {
        self.configured_telegram_allowed_username
            .as_deref()
            .is_none_or(|configured_username| {
                sender_username.is_some_and(|incoming_username| {
                    incoming_username.eq_ignore_ascii_case(configured_username)
                })
            })
    }

    #[must_use]
    pub fn is_update_authorized(
        &self,
        chat_identifier: i64,
        sender_username: Option<&str>,
    ) -> bool {
        self.is_chat_authorized(chat_identifier) && self.is_sender_authorized(sender_username)
    }

    #[must_use]
    pub fn metrics(&self) -> Arc<ServiceMetrics> {
        Arc::clone(&self.metrics)
    }

    #[must_use]
    pub fn new(
        telegram_api_client: TelegramApiClient,
        configured_telegram_admin_usernames: Vec<String>,
        configured_telegram_allowed_username: Option<String>,
        configured_telegram_chat_identifier: Option<i64>,
        codex_max_parallel_tasks: usize,
        update_processing_max_parallel_tasks: usize,
        task_manager: TaskManager,
    ) -> Self {
        Self {
            codex_semaphore: Arc::new(Semaphore::new(codex_max_parallel_tasks)),
            configured_telegram_admin_usernames,
            configured_telegram_allowed_username,
            configured_telegram_chat_identifier,
            correlation_identifier_counter: Arc::new(AtomicU64::new(1)),
            metrics: Arc::new(ServiceMetrics::new()),
            polling_is_ready: Arc::new(AtomicBool::new(false)),
            task_manager,
            telegram_api_client,
            update_processing_semaphore: Arc::new(Semaphore::new(
                update_processing_max_parallel_tasks,
            )),
        }
    }

    #[must_use]
    pub fn next_correlation_identifier(&self) -> String {
        let current_identifier = self
            .correlation_identifier_counter
            .fetch_add(1, Ordering::Relaxed);
        format!("correlation-{current_identifier}")
    }

    pub fn set_polling_ready(&self, polling_is_ready: bool) {
        self.polling_is_ready
            .store(polling_is_ready, Ordering::SeqCst);
    }

    #[must_use]
    pub const fn task_manager(&self) -> &TaskManager {
        &self.task_manager
    }

    #[must_use]
    pub const fn telegram_client(&self) -> &TelegramApiClient {
        &self.telegram_api_client
    }

    pub fn try_acquire_codex_permit(&self) -> Result<OwnedSemaphorePermit, TryAcquireError> {
        Arc::clone(&self.codex_semaphore).try_acquire_owned()
    }

    pub fn try_acquire_update_processing_permit(
        &self,
    ) -> Result<OwnedSemaphorePermit, TryAcquireError> {
        Arc::clone(&self.update_processing_semaphore).try_acquire_owned()
    }
}
