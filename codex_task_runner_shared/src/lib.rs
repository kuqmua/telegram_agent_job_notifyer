mod cfg;
mod fs_ops;
mod runner;
mod types;

use std::{
    env, fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use types::ProgressEntry;

pub const DEFAULT_LOG_MAXIMUM_BYTES: usize = 65_536usize;
pub const DEFAULT_MANAGED_DIRECTORY_NAME: &str = "cdx_cli_manage";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskRunnerConfiguration {
    pub codex_binary_path: String,
    pub log_maximum_bytes: usize,
    pub managed_directory_path: PathBuf,
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
    let progress_entries = Arc::new(Mutex::new(Vec::<ProgressEntry>::new()));
    let mut task_index = 0usize;
    while application_configuration.tasks.get(task_index).is_some() {
        progress_entries
            .lock()
            .map_err(|error| {
                format!("7a1c3e5f failed to lock progress entries for initialization: {error}")
            })?
            .push(ProgressEntry {
                current_iteration: 0u32,
                finished: false,
                iteration_started_at: None,
            });
        task_index = task_index.saturating_add(1usize);
    }
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
    run_result.map_err(|error| format!("8f1b3d5a one or more execution stages failed: {error}"))
}
