use std::{
    collections::{BTreeMap, VecDeque},
    fs::{OpenOptions, read_to_string},
    io::Write as _,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::{
    settings::TaskHistoryFilePath,
    shared::{
        CodexTaskStatus, PromptText, TaskCreationRequest, TaskExecutionOutputText, TaskOwner,
        TaskSummary,
    },
};

#[derive(Copy, Clone, Debug)]
pub enum TaskCancellationResult {
    AccessDenied,
    AlreadyTerminal,
    Cancelled,
    NotFound,
}

#[derive(Copy, Clone, Debug)]
pub enum TaskCreationError {
    PromptTooLong {
        maximum_characters: usize,
        prompt_characters: usize,
    },
    RateLimited,
}

#[derive(Copy, Clone, Debug)]
pub enum TaskLookupError {
    AccessDenied,
    NotFound,
}

#[derive(Debug)]
pub enum TaskRetryLookup {
    AccessDenied,
    NotFound,
    Ready(TaskCreationRequest),
}

#[derive(Debug, Serialize, Deserialize)]
struct TaskHistorySnapshot {
    created_unix_milliseconds: u64,
    finished_unix_milliseconds: Option<u64>,
    owner: TaskOwner,
    started_unix_milliseconds: Option<u64>,
    status: CodexTaskStatus,
    task_identifier: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct TaskRecordSnapshot {
    created_unix_milliseconds: u64,
    finished_unix_milliseconds: Option<u64>,
    owner: TaskOwner,
    prompt_text: PromptText,
    result_text: Option<TaskExecutionOutputText>,
    started_unix_milliseconds: Option<u64>,
    status: CodexTaskStatus,
    task_identifier: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct TaskRegistrySnapshot {
    completed_task_identifiers: Vec<u64>,
    next_task_identifier: u64,
    task_records: Vec<TaskRecordSnapshot>,
}

#[derive(Debug)]
struct TaskRecord {
    cancellation_flag: Arc<AtomicBool>,
    created_unix_milliseconds: u64,
    finished_unix_milliseconds: Option<u64>,
    owner: TaskOwner,
    prompt_text: PromptText,
    result_text: Option<TaskExecutionOutputText>,
    started_unix_milliseconds: Option<u64>,
    status: CodexTaskStatus,
    task_identifier: u64,
}

impl TaskRecord {
    fn is_owner(&self, owner_chat_identifier: i64, owner_sender_username: Option<&str>) -> bool {
        if self.owner.chat_identifier != owner_chat_identifier {
            return false;
        }
        match (&self.owner.sender_username, owner_sender_username) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(expected_username), Some(incoming_username)) => {
                expected_username.eq_ignore_ascii_case(incoming_username)
            }
        }
    }

    fn summary(&self) -> TaskSummary {
        TaskSummary {
            created_unix_milliseconds: self.created_unix_milliseconds,
            finished_unix_milliseconds: self.finished_unix_milliseconds,
            owner: self.owner.clone(),
            started_unix_milliseconds: self.started_unix_milliseconds,
            status: self.status,
            task_identifier: self.task_identifier,
        }
    }
}

#[derive(Debug, Default)]
struct TaskRegistry {
    completed_task_identifiers: VecDeque<u64>,
    task_records: BTreeMap<u64, TaskRecord>,
    task_windows_by_owner_key: BTreeMap<String, VecDeque<u64>>,
}

#[derive(Clone, Debug)]
struct TaskRegistryStateFilePath(String);

impl From<String> for TaskRegistryStateFilePath {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Clone, Debug)]
pub struct TaskManager {
    history_file_path: Option<TaskHistoryFilePath>,
    history_maximum_size: usize,
    next_task_identifier: Arc<AtomicU64>,
    prompt_maximum_characters: usize,
    rate_limit_per_minute: usize,
    registry: Arc<Mutex<TaskRegistry>>,
    state_file_path: Option<TaskRegistryStateFilePath>,
}

impl TaskManager {
    fn append_history_snapshot(&self, history_snapshot: &TaskHistorySnapshot) {
        let Some(history_file_path) = &self.history_file_path else {
            return;
        };
        let open_result = OpenOptions::new()
            .append(true)
            .create(true)
            .open(history_file_path.as_str());
        let Ok(mut history_file) = open_result else {
            return;
        };
        let serialize_result = serde_json::to_string(history_snapshot);
        let Ok(serialized_line) = serialize_result else {
            return;
        };
        let write_result = writeln!(history_file, "{serialized_line}");
        if write_result.is_err() {
            return;
        }
        let _flush_result = history_file.flush();
    }

    pub async fn create_task(
        &self,
        task_creation_request: TaskCreationRequest,
    ) -> Result<u64, TaskCreationError> {
        let prompt_character_count = task_creation_request.prompt_text.character_count();
        if prompt_character_count > self.prompt_maximum_characters {
            return Err(TaskCreationError::PromptTooLong {
                maximum_characters: self.prompt_maximum_characters,
                prompt_characters: prompt_character_count,
            });
        }
        let created_unix_milliseconds = now_unix_milliseconds();
        let mut registry_guard = self.registry.lock().await;
        let normalized_sender_username = task_creation_request
            .owner
            .sender_username
            .as_deref()
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        let owner_key = format!(
            "{}:{normalized_sender_username}",
            task_creation_request.owner.chat_identifier,
        );
        let owner_window = registry_guard
            .task_windows_by_owner_key
            .entry(owner_key)
            .or_insert_with(VecDeque::new);
        let one_minute_milliseconds = 60_000u64;
        let window_start_milliseconds =
            created_unix_milliseconds.saturating_sub(one_minute_milliseconds);
        while owner_window
            .front()
            .is_some_and(|timestamp| *timestamp < window_start_milliseconds)
        {
            let _removed_value = owner_window.pop_front();
        }
        if owner_window.len() >= self.rate_limit_per_minute {
            drop(registry_guard);
            return Err(TaskCreationError::RateLimited);
        }
        owner_window.push_back(created_unix_milliseconds);
        let task_identifier = self.next_task_identifier.fetch_add(1, Ordering::Relaxed);
        let task_record = TaskRecord {
            cancellation_flag: Arc::new(AtomicBool::new(false)),
            created_unix_milliseconds,
            finished_unix_milliseconds: None,
            owner: task_creation_request.owner,
            prompt_text: task_creation_request.prompt_text,
            result_text: None,
            started_unix_milliseconds: None,
            status: CodexTaskStatus::Queued,
            task_identifier,
        };
        let _previous_task = registry_guard
            .task_records
            .insert(task_identifier, task_record);
        self.persist_registry_snapshot(&registry_guard);
        drop(registry_guard);
        Ok(task_identifier)
    }

    pub async fn get_retry_task_creation_request(
        &self,
        task_identifier: u64,
        requester_chat_identifier: i64,
        requester_sender_username: Option<&str>,
        requester_is_administrator: bool,
    ) -> TaskRetryLookup {
        let registry_guard = self.registry.lock().await;
        let Some(task_record) = registry_guard.task_records.get(&task_identifier) else {
            drop(registry_guard);
            return TaskRetryLookup::NotFound;
        };
        if !requester_is_administrator
            && !task_record.is_owner(requester_chat_identifier, requester_sender_username)
        {
            drop(registry_guard);
            return TaskRetryLookup::AccessDenied;
        }
        let task_creation_request = TaskCreationRequest {
            owner: task_record.owner.clone(),
            prompt_text: task_record.prompt_text.clone(),
        };
        drop(registry_guard);
        TaskRetryLookup::Ready(task_creation_request)
    }

    pub async fn get_task_cancellation_flag(
        &self,
        task_identifier: u64,
    ) -> Result<Arc<AtomicBool>, TaskLookupError> {
        let registry_guard = self.registry.lock().await;
        let task_record = registry_guard
            .task_records
            .get(&task_identifier)
            .ok_or(TaskLookupError::NotFound)?;
        let cancellation_flag = Arc::clone(&task_record.cancellation_flag);
        drop(registry_guard);
        Ok(cancellation_flag)
    }

    pub async fn get_task_output(
        &self,
        task_identifier: u64,
        requester_chat_identifier: i64,
        requester_sender_username: Option<&str>,
        requester_is_administrator: bool,
    ) -> Result<Option<TaskExecutionOutputText>, TaskLookupError> {
        let registry_guard = self.registry.lock().await;
        let task_record = registry_guard
            .task_records
            .get(&task_identifier)
            .ok_or(TaskLookupError::NotFound)?;
        if !requester_is_administrator
            && !task_record.is_owner(requester_chat_identifier, requester_sender_username)
        {
            drop(registry_guard);
            return Err(TaskLookupError::AccessDenied);
        }
        let result_text = task_record.result_text.clone();
        drop(registry_guard);
        Ok(result_text)
    }

    pub async fn get_task_prompt_for_execution(
        &self,
        task_identifier: u64,
    ) -> Result<PromptText, TaskLookupError> {
        let registry_guard = self.registry.lock().await;
        let task_record = registry_guard
            .task_records
            .get(&task_identifier)
            .ok_or(TaskLookupError::NotFound)?;
        let prompt_text = task_record.prompt_text.clone();
        drop(registry_guard);
        Ok(prompt_text)
    }

    pub async fn get_task_summary(
        &self,
        task_identifier: u64,
        requester_chat_identifier: i64,
        requester_sender_username: Option<&str>,
        requester_is_administrator: bool,
    ) -> Result<TaskSummary, TaskLookupError> {
        let registry_guard = self.registry.lock().await;
        let task_record = registry_guard
            .task_records
            .get(&task_identifier)
            .ok_or(TaskLookupError::NotFound)?;
        if !requester_is_administrator
            && !task_record.is_owner(requester_chat_identifier, requester_sender_username)
        {
            drop(registry_guard);
            return Err(TaskLookupError::AccessDenied);
        }
        let summary = task_record.summary();
        drop(registry_guard);
        Ok(summary)
    }

    pub async fn list_active_tasks(
        &self,
        requester_chat_identifier: i64,
        requester_sender_username: Option<&str>,
        requester_is_administrator: bool,
        maximum_items: usize,
    ) -> Vec<TaskSummary> {
        let registry_guard = self.registry.lock().await;
        let summaries = registry_guard
            .task_records
            .iter()
            .rev()
            .filter_map(|(_task_identifier, task_record)| {
                if task_record.status.is_terminal() {
                    return None;
                }
                let is_visible = requester_is_administrator
                    || task_record.is_owner(requester_chat_identifier, requester_sender_username);
                if !is_visible {
                    return None;
                }
                Some(task_record.summary())
            })
            .take(maximum_items)
            .collect();
        drop(registry_guard);
        summaries
    }

    pub async fn list_recent_tasks(
        &self,
        requester_chat_identifier: i64,
        requester_sender_username: Option<&str>,
        requester_is_administrator: bool,
        maximum_items: usize,
    ) -> Vec<TaskSummary> {
        let registry_guard = self.registry.lock().await;
        let summaries = registry_guard
            .task_records
            .iter()
            .rev()
            .filter_map(|(_task_identifier, task_record)| {
                let is_visible = requester_is_administrator
                    || task_record.is_owner(requester_chat_identifier, requester_sender_username);
                if !is_visible {
                    return None;
                }
                Some(task_record.summary())
            })
            .take(maximum_items)
            .collect();
        drop(registry_guard);
        summaries
    }

    pub async fn mark_task_cancelled(&self, task_identifier: u64) -> Result<(), TaskLookupError> {
        self.mark_task_terminal(task_identifier, CodexTaskStatus::Cancelled, None, None)
            .await
    }

    pub async fn mark_task_failed(
        &self,
        task_identifier: u64,
        error_text: TaskExecutionOutputText,
    ) -> Result<(), TaskLookupError> {
        self.mark_task_terminal(task_identifier, CodexTaskStatus::Failed, None, Some(error_text))
            .await
    }

    pub async fn mark_task_running(&self, task_identifier: u64) -> Result<(), TaskLookupError> {
        let mut registry_guard = self.registry.lock().await;
        let task_record = registry_guard
            .task_records
            .get_mut(&task_identifier)
            .ok_or(TaskLookupError::NotFound)?;
        if task_record.status.is_terminal() {
            drop(registry_guard);
            return Ok(());
        }
        task_record.status = CodexTaskStatus::Running;
        task_record.started_unix_milliseconds = Some(now_unix_milliseconds());
        self.persist_registry_snapshot(&registry_guard);
        drop(registry_guard);
        Ok(())
    }

    pub async fn mark_task_succeeded(
        &self,
        task_identifier: u64,
        result_text: TaskExecutionOutputText,
    ) -> Result<(), TaskLookupError> {
        self.mark_task_terminal(
            task_identifier,
            CodexTaskStatus::Succeeded,
            Some(result_text),
            None,
        )
        .await
    }

    async fn mark_task_terminal(
        &self,
        task_identifier: u64,
        status: CodexTaskStatus,
        result_text: Option<TaskExecutionOutputText>,
        error_text: Option<TaskExecutionOutputText>,
    ) -> Result<(), TaskLookupError> {
        let mut registry_guard = self.registry.lock().await;
        let history_snapshot = {
            let task_record = registry_guard
                .task_records
                .get_mut(&task_identifier)
                .ok_or(TaskLookupError::NotFound)?;
            if task_record.status.is_terminal() {
                drop(registry_guard);
                return Ok(());
            }
            task_record.status = status;
            task_record.result_text = result_text.or(error_text);
            task_record.finished_unix_milliseconds = Some(now_unix_milliseconds());
            TaskHistorySnapshot {
                created_unix_milliseconds: task_record.created_unix_milliseconds,
                finished_unix_milliseconds: task_record.finished_unix_milliseconds,
                owner: task_record.owner.clone(),
                started_unix_milliseconds: task_record.started_unix_milliseconds,
                status: task_record.status,
                task_identifier: task_record.task_identifier,
            }
        };
        registry_guard
            .completed_task_identifiers
            .push_back(task_identifier);
        while registry_guard.task_records.len() > self.history_maximum_size {
            let Some(oldest_task_identifier) =
                registry_guard.completed_task_identifiers.pop_front()
            else {
                break;
            };
            let _removed_task = registry_guard.task_records.remove(&oldest_task_identifier);
        }
        self.persist_registry_snapshot(&registry_guard);
        drop(registry_guard);
        self.append_history_snapshot(&history_snapshot);
        Ok(())
    }

    pub async fn mark_task_timed_out(&self, task_identifier: u64) -> Result<(), TaskLookupError> {
        self.mark_task_terminal(task_identifier, CodexTaskStatus::TimedOut, None, None)
            .await
    }

    #[must_use]
    pub fn new(
        history_file_path: Option<TaskHistoryFilePath>,
        history_maximum_size: usize,
        prompt_maximum_characters: usize,
        rate_limit_per_minute: usize,
    ) -> Self {
        let state_file_path = history_file_path
            .as_ref()
            .map(TaskHistoryFilePath::as_str)
            .map(|path_value| format!("{path_value}.state.json"))
            .map(TaskRegistryStateFilePath::from);
        let loaded_snapshot = state_file_path
            .as_ref()
            .map(|state_file_path_value| state_file_path_value.0.as_str())
            .and_then(|state_file_path_value| read_to_string(state_file_path_value).ok())
            .and_then(|serialized_snapshot| {
                serde_json::from_str::<TaskRegistrySnapshot>(&serialized_snapshot).ok()
            });
        let mut restored_registry = TaskRegistry::default();
        let mut restored_next_task_identifier = 1u64;
        if let Some(task_registry_snapshot) = loaded_snapshot {
            restored_next_task_identifier = task_registry_snapshot.next_task_identifier.max(1);
            restored_registry.completed_task_identifiers =
                task_registry_snapshot.completed_task_identifiers.into();
            for task_record_snapshot in task_registry_snapshot.task_records {
                let normalized_sender_username = task_record_snapshot
                    .owner
                    .sender_username
                    .as_deref()
                    .map(str::to_ascii_lowercase)
                    .unwrap_or_default();
                let owner_key = format!(
                    "{}:{normalized_sender_username}",
                    task_record_snapshot.owner.chat_identifier
                );
                let task_record = TaskRecord {
                    cancellation_flag: Arc::new(AtomicBool::new(false)),
                    created_unix_milliseconds: task_record_snapshot.created_unix_milliseconds,
                    finished_unix_milliseconds: task_record_snapshot.finished_unix_milliseconds,
                    owner: task_record_snapshot.owner,
                    prompt_text: task_record_snapshot.prompt_text,
                    result_text: task_record_snapshot.result_text,
                    started_unix_milliseconds: if task_record_snapshot.status
                        == CodexTaskStatus::Running
                    {
                        None
                    } else {
                        task_record_snapshot.started_unix_milliseconds
                    },
                    status: if task_record_snapshot.status == CodexTaskStatus::Running {
                        CodexTaskStatus::Queued
                    } else {
                        task_record_snapshot.status
                    },
                    task_identifier: task_record_snapshot.task_identifier,
                };
                restored_registry
                    .task_windows_by_owner_key
                    .entry(owner_key)
                    .or_insert_with(VecDeque::new)
                    .push_back(task_record.created_unix_milliseconds);
                let _previous_task = restored_registry
                    .task_records
                    .insert(task_record.task_identifier, task_record);
            }
        }
        Self {
            history_file_path,
            history_maximum_size,
            next_task_identifier: Arc::new(AtomicU64::new(restored_next_task_identifier)),
            prompt_maximum_characters,
            rate_limit_per_minute,
            registry: Arc::new(Mutex::new(restored_registry)),
            state_file_path,
        }
    }

    fn persist_registry_snapshot(&self, registry: &TaskRegistry) {
        let Some(state_file_path) = &self.state_file_path else {
            return;
        };
        let task_records = registry
            .task_records
            .values()
            .map(|task_record| TaskRecordSnapshot {
                created_unix_milliseconds: task_record.created_unix_milliseconds,
                finished_unix_milliseconds: task_record.finished_unix_milliseconds,
                owner: task_record.owner.clone(),
                prompt_text: task_record.prompt_text.clone(),
                result_text: task_record.result_text.clone(),
                started_unix_milliseconds: task_record.started_unix_milliseconds,
                status: task_record.status,
                task_identifier: task_record.task_identifier,
            })
            .collect::<Vec<_>>();
        let snapshot = TaskRegistrySnapshot {
            completed_task_identifiers: registry
                .completed_task_identifiers
                .iter()
                .copied()
                .collect(),
            next_task_identifier: self.next_task_identifier.load(Ordering::Relaxed),
            task_records,
        };
        let Ok(serialized_snapshot) = serde_json::to_string(&snapshot) else {
            return;
        };
        let open_result = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(state_file_path.0.as_str());
        let Ok(mut state_file) = open_result else {
            return;
        };
        if state_file
            .write_all(serialized_snapshot.as_bytes())
            .is_err()
        {
            return;
        }
        let _flush_result = state_file.flush();
    }

    pub async fn queued_task_dispatch_data(&self) -> Vec<(u64, i64)> {
        let registry_guard = self.registry.lock().await;
        let queued_task_dispatch_data = registry_guard
            .task_records
            .iter()
            .filter_map(|(task_identifier, task_record)| {
                if task_record.status != CodexTaskStatus::Queued {
                    return None;
                }
                Some((*task_identifier, task_record.owner.chat_identifier))
            })
            .collect::<Vec<_>>();
        drop(registry_guard);
        queued_task_dispatch_data
    }

    pub async fn request_task_cancellation(
        &self,
        task_identifier: u64,
        requester_chat_identifier: i64,
        requester_sender_username: Option<&str>,
        requester_is_administrator: bool,
    ) -> TaskCancellationResult {
        let mut registry_guard = self.registry.lock().await;
        let Some(task_record) = registry_guard.task_records.get_mut(&task_identifier) else {
            drop(registry_guard);
            return TaskCancellationResult::NotFound;
        };
        if !requester_is_administrator
            && !task_record.is_owner(requester_chat_identifier, requester_sender_username)
        {
            drop(registry_guard);
            return TaskCancellationResult::AccessDenied;
        }
        if task_record.status.is_terminal() {
            drop(registry_guard);
            return TaskCancellationResult::AlreadyTerminal;
        }
        task_record.cancellation_flag.store(true, Ordering::Relaxed);
        if task_record.status == CodexTaskStatus::Queued {
            task_record.status = CodexTaskStatus::Cancelled;
            task_record.finished_unix_milliseconds = Some(now_unix_milliseconds());
            registry_guard
                .completed_task_identifiers
                .push_back(task_identifier);
            while registry_guard.task_records.len() > self.history_maximum_size {
                let Some(oldest_task_identifier) =
                    registry_guard.completed_task_identifiers.pop_front()
                else {
                    break;
                };
                let _removed_task = registry_guard.task_records.remove(&oldest_task_identifier);
            }
        }
        self.persist_registry_snapshot(&registry_guard);
        drop(registry_guard);
        TaskCancellationResult::Cancelled
    }

    pub async fn task_queue_depth(&self) -> u64 {
        let registry_guard = self.registry.lock().await;
        let queued_task_count = registry_guard
            .task_records
            .values()
            .filter(|task_record| task_record.status == CodexTaskStatus::Queued)
            .count();
        drop(registry_guard);
        u64::try_from(queued_task_count).unwrap_or(u64::MAX)
    }

    pub async fn task_queue_running_depth(&self) -> (u64, u64) {
        let registry_guard = self.registry.lock().await;
        let mut queued_task_count = 0u64;
        let mut running_task_count = 0u64;
        for task_record in registry_guard.task_records.values() {
            if task_record.status == CodexTaskStatus::Queued {
                queued_task_count = queued_task_count.saturating_add(1);
            }
            if task_record.status == CodexTaskStatus::Running {
                running_task_count = running_task_count.saturating_add(1);
            }
        }
        drop(registry_guard);
        (queued_task_count, running_task_count)
    }
}

fn now_unix_milliseconds() -> u64 {
    let duration_since_epoch = SystemTime::now().duration_since(UNIX_EPOCH);
    duration_since_epoch
        .map_or(0u64, |duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        TaskCancellationResult, TaskCreationError, TaskLookupError, TaskManager, TaskRetryLookup,
    };
    use crate::{
        settings::TaskHistoryFilePath,
        shared::{CodexTaskStatus, PromptText, SenderUsername, TaskCreationRequest, TaskOwner},
    };

    #[tokio::test]
    async fn create_and_read_task_summary() {
        let task_manager = TaskManager::new(None, 128, 8_000, 100);
        let task_identifier = task_manager
            .create_task(TaskCreationRequest {
                owner: TaskOwner {
                    chat_identifier: 11,
                    sender_username: Some(SenderUsername::from(String::from("tester"))),
                },
                prompt_text: PromptText::from(String::from("explain ownership")),
            })
            .await
            .expect("ebf13d02");
        let task_summary = task_manager
            .get_task_summary(task_identifier, 11, Some("tester"), false)
            .await
            .expect("ec1d80b1");
        assert_eq!(task_summary.task_identifier, task_identifier);
    }

    #[tokio::test]
    async fn queued_task_is_cancelled_before_run() {
        let task_manager = TaskManager::new(None, 128, 8_000, 100);
        let task_identifier = task_manager
            .create_task(TaskCreationRequest {
                owner: TaskOwner {
                    chat_identifier: 11,
                    sender_username: None,
                },
                prompt_text: PromptText::from(String::from("explain ownership")),
            })
            .await
            .expect("b39a09a7");
        let cancellation_result = task_manager
            .request_task_cancellation(task_identifier, 11, None, false)
            .await;
        assert!(matches!(cancellation_result, TaskCancellationResult::Cancelled));
        let task_summary = task_manager
            .get_task_summary(task_identifier, 11, None, false)
            .await
            .expect("adf702f0");
        assert_eq!(task_summary.status, CodexTaskStatus::Cancelled);
    }

    #[tokio::test]
    async fn rate_limit_blocks_excessive_task_creation() {
        let task_manager = TaskManager::new(None, 128, 8_000, 1);
        let first_creation_result = task_manager
            .create_task(TaskCreationRequest {
                owner: TaskOwner {
                    chat_identifier: 11,
                    sender_username: None,
                },
                prompt_text: PromptText::from(String::from("first")),
            })
            .await;
        let created_task_identifier = first_creation_result.expect("f8c2d1e4");
        assert_eq!(created_task_identifier, 1);
        let second_creation_result = task_manager
            .create_task(TaskCreationRequest {
                owner: TaskOwner {
                    chat_identifier: 11,
                    sender_username: None,
                },
                prompt_text: PromptText::from(String::from("second")),
            })
            .await;
        assert!(matches!(second_creation_result, Err(TaskCreationError::RateLimited)));
    }

    #[tokio::test]
    async fn retry_lookup_returns_prompt() {
        let task_manager = TaskManager::new(None, 128, 8_000, 100);
        let task_identifier = task_manager
            .create_task(TaskCreationRequest {
                owner: TaskOwner {
                    chat_identifier: 11,
                    sender_username: None,
                },
                prompt_text: PromptText::from(String::from("hello")),
            })
            .await
            .expect("cc84f522");
        let retry_lookup = task_manager
            .get_retry_task_creation_request(task_identifier, 11, None, false)
            .await;
        assert!(matches!(retry_lookup, TaskRetryLookup::Ready(_)));
    }

    #[tokio::test]
    async fn task_access_is_denied_for_other_owner() {
        let task_manager = TaskManager::new(None, 128, 8_000, 100);
        let task_identifier = task_manager
            .create_task(TaskCreationRequest {
                owner: TaskOwner {
                    chat_identifier: 11,
                    sender_username: Some(SenderUsername::from(String::from("tester"))),
                },
                prompt_text: PromptText::from(String::from("explain ownership")),
            })
            .await
            .expect("db8f4f72");
        let read_result = task_manager
            .get_task_summary(task_identifier, 22, Some("another"), false)
            .await;
        assert!(matches!(read_result, Err(TaskLookupError::AccessDenied)));
    }

    #[tokio::test]
    async fn prompt_too_long_is_rejected_before_queue() {
        let task_manager = TaskManager::new(None, 128, 5, 100);
        let creation_result = task_manager
            .create_task(TaskCreationRequest {
                owner: TaskOwner {
                    chat_identifier: 11,
                    sender_username: Some(SenderUsername::from(String::from("tester"))),
                },
                prompt_text: PromptText::from(String::from("123456")),
            })
            .await;
        assert!(matches!(
            creation_result,
            Err(TaskCreationError::PromptTooLong {
                maximum_characters: 5,
                prompt_characters: 6
            })
        ));
        assert_eq!(task_manager.task_queue_depth().await, 0);
    }

    #[tokio::test]
    async fn queued_tasks_are_restored_from_snapshot_file() {
        let random_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0u128, |duration| duration.as_nanos());
        let history_file_path = env::temp_dir().join(format!("task-history-{random_suffix}.jsonl"));
        let history_file_string = history_file_path.to_string_lossy().into_owned();
        let task_manager = TaskManager::new(
            Some(TaskHistoryFilePath::from(history_file_string.clone())),
            128,
            8_000,
            100,
        );
        let created_task_identifier = task_manager
            .create_task(TaskCreationRequest {
                owner: TaskOwner {
                    chat_identifier: 11,
                    sender_username: Some(SenderUsername::from(String::from("tester"))),
                },
                prompt_text: PromptText::from(String::from("restore me")),
            })
            .await
            .expect("a4b9d1f3");
        assert_eq!(created_task_identifier, 1);
        drop(task_manager);
        let restored_task_manager = TaskManager::new(
            Some(TaskHistoryFilePath::from(history_file_string.clone())),
            128,
            8_000,
            100,
        );
        let queued_task_dispatch_data = restored_task_manager.queued_task_dispatch_data().await;
        assert_eq!(queued_task_dispatch_data.len(), 1);
        let first_dispatch_data = queued_task_dispatch_data.first().expect("c2e8f4a1");
        assert_eq!(first_dispatch_data.0, 1);
        assert_eq!(first_dispatch_data.1, 11);
        assert_eq!(restored_task_manager.task_queue_depth().await, 1);
        let state_file_path = format!("{history_file_string}.state.json");
        let _remove_state_result = fs::remove_file(state_file_path);
        let _remove_history_result = fs::remove_file(history_file_string);
    }
}
