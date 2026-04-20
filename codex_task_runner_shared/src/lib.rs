mod cfg;
mod fs_ops;
mod runner;
mod status;
mod types;
mod web;

use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use types::ProgressEntry;

pub const DEFAULT_LOG_MAXIMUM_BYTES: usize = 65_536usize;
pub const DEFAULT_TASKS_FILE_NAME: &str = "tasks.json";
pub const DEFAULT_MANAGED_DIRECTORY_NAME: &str = "cdx_cli_manage";
pub const DEFAULT_LOG_VIEWER_BIND_ADDRESS: &str = "127.0.0.1:7879";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskRunnerConfiguration {
    pub codex_binary_path: String,
    pub log_maximum_bytes: usize,
    pub log_viewer_bind_address: Option<String>,
    pub managed_directory_path: PathBuf,
    pub status_reporting_enabled: bool,
}

impl TaskRunnerConfiguration {
    #[must_use]
    pub fn cli_default(codex_binary_path: String, log_maximum_bytes: usize) -> Self {
        Self {
            codex_binary_path,
            log_maximum_bytes,
            log_viewer_bind_address: Some(String::from(DEFAULT_LOG_VIEWER_BIND_ADDRESS)),
            managed_directory_path: PathBuf::from(DEFAULT_MANAGED_DIRECTORY_NAME),
            status_reporting_enabled: true,
        }
    }
}

#[must_use]
pub fn resolve_codex_binary_from_environment() -> String {
    match env::var("CODEX_BIN") {
        Ok(value) if !value.trim().is_empty() => value,
        Ok(_) | Err(_) => String::from("codex"),
    }
}

pub fn resolve_log_maximum_bytes_from_environment() -> Result<usize, String> {
    let parsed_value = match env::var("CDX_LOG_MAX_BYTES") {
        Ok(value) if !value.trim().is_empty() => value.parse::<usize>().map_err(|error| {
            format!("9d4b6c8e CDX_LOG_MAX_BYTES must be a positive integer: {error}")
        })?,
        Ok(_) | Err(_) => DEFAULT_LOG_MAXIMUM_BYTES,
    };
    if parsed_value == 0usize {
        return Err(String::from("0e5c7d9f CDX_LOG_MAX_BYTES must be greater than 0"));
    }
    Ok(parsed_value)
}

pub fn run_tasks_json(
    tasks_json: &str,
    task_runner_configuration: &TaskRunnerConfiguration,
) -> Result<(), String> {
    fs::create_dir_all(task_runner_configuration.managed_directory_path.as_path()).map_err(
        |error| {
            format!(
                "3d7f9a1c failed to create managed directory `{}`: {error}",
                task_runner_configuration.managed_directory_path.display()
            )
        },
    )?;
    let application_configuration = cfg::parse_cfg(tasks_json)?;
    fs_ops::clear_dir(task_runner_configuration.managed_directory_path.as_path())?;
    let log_viewer = task_runner_configuration
        .log_viewer_bind_address
        .as_deref()
        .map(|bind_address| {
            web::start_log_viewer(
                bind_address,
                task_runner_configuration.managed_directory_path.as_path(),
            )
        })
        .transpose()?;
    let progress_entries = Arc::new(Mutex::new(Vec::<ProgressEntry>::new()));
    let mut task_index = 0usize;
    while let Some(task_specification) = application_configuration.tasks.get(task_index) {
        let task_started_at_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("6d0f2a4c failed to compute task start unix time: {error}"))?
            .as_secs();
        progress_entries
            .lock()
            .map_err(|error| {
                format!("7a1c3e5f failed to lock progress entries for initialization: {error}")
            })?
            .push(ProgressEntry {
                current_iteration: 0u32,
                finished: false,
                iteration_started_at: None,
                prompt: task_specification.prompt.clone(),
                task_started_at_unix,
                total_repeat: task_specification.repeat,
            });
        task_index = task_index.saturating_add(1usize);
    }
    let status_reporter = task_runner_configuration
        .status_reporting_enabled
        .then(|| status::start_status_reporter(Arc::clone(&progress_entries)));
    let mut stop_configuration_task_index = 0usize;
    while let Some(task_specification) = application_configuration
        .tasks
        .get(stop_configuration_task_index)
    {
        let suffix = stop_configuration_task_index.saturating_add(1usize);
        let configuration_file_path =
            task_runner_configuration
                .managed_directory_path
                .join(format!(
                    "{}_{}_cdx_cli.json",
                    fs_ops::prompt_based_name(task_specification.prompt.as_str()),
                    suffix
                ));
        fs::write(configuration_file_path.as_path(), "{\"stop\": false}\n").map_err(|error| {
            format!(
                "2f7b9c1d failed to create default stop config file `{}`: {error}",
                configuration_file_path.display()
            )
        })?;
        stop_configuration_task_index = stop_configuration_task_index.saturating_add(1usize);
    }
    let run_result = runner::run_tasks(
        application_configuration.tasks.as_slice(),
        task_runner_configuration.codex_binary_path.as_str(),
        task_runner_configuration.managed_directory_path.as_path(),
        task_runner_configuration.log_maximum_bytes,
        &progress_entries,
    );
    let status_result = status_reporter.map(status::StatusReporter::stop);
    let log_viewer_result = log_viewer.map(web::LogViewer::stop);
    let mut errors = Vec::<String>::new();
    if let Err(error) = run_result {
        errors.push(error);
    }
    if let Some(Err(error)) = status_result {
        errors.push(error);
    }
    if let Some(Err(error)) = log_viewer_result {
        errors.push(error);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("8f1b3d5a one or more execution stages failed: {}", errors.join(" | ")))
    }
}

pub fn run_tasks_json_file(
    tasks_file_path: &Path,
    task_runner_configuration: &TaskRunnerConfiguration,
) -> Result<(), String> {
    let tasks_json = fs::read_to_string(tasks_file_path).map_err(|error| {
        format!(
            "2e7a9c4d failed to read tasks file `{}`. Expected JSON array, example: \
             [{{\"prompt\":\"создай простой html\",\"repeat\":2}},{{\"prompt\":\"создай \
             README\",\"repeat\":1}}]. Source: {error}",
            tasks_file_path.display()
        )
    })?;
    run_tasks_json(tasks_json.as_str(), task_runner_configuration)
}
