mod cfg;
mod fs_ops;
mod runner;
mod status;
mod types;
mod web;

use std::{
    env, fs,
    io::{Write as _, stderr},
    path::PathBuf,
    process::ExitCode,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use types::ProgressEntry;

const CDX_DIR_NAME: &str = "cdx_cli_manage";
const DEFAULT_LOG_MAX_BYTES: usize = 65_536usize;
const HARD_CODED_STATUS_SERVER_ADDRESS: &str = "127.0.0.1:7879";

fn main() -> ExitCode {
    match (|| -> Result<(), String> {
        let mut args = env::args_os();
        let _bin = args.next();
        let tasks_path = args.next().map_or_else(
            || Ok(PathBuf::from("tasks.json")),
            |tasks_file_os| {
                if args.next().is_some() {
                    return Err(String::from(
                        "6b0d3f5a at most 1 argument is allowed: <tasks-json-file>",
                    ));
                }
                Ok(PathBuf::from(tasks_file_os))
            },
        )?;
        fs::create_dir_all(CDX_DIR_NAME).map_err(|error| {
            format!("3d7f9a1c failed to create cdx_cli_manage directory: {error}")
        })?;
        let tasks_json = fs::read_to_string(tasks_path.as_path()).map_err(|error| {
            format!(
                "2e7a9c4d failed to read tasks file `{}`. Expected JSON object, example: \
                 {{\"server\":\"127.0.0.1:7878\",\"tasks\":[{{\"prompt\":\"создай простой \
                 html\",\"repeat\":2}},{{\"prompt\":\"создай README\",\"repeat\":1}}]}}. Source: \
                 {error}",
                tasks_path.display()
            )
        })?;
        let app_cfg = cfg::parse_cfg(tasks_json.as_str())?;
        let cdx_dir = PathBuf::from(CDX_DIR_NAME);
        fs_ops::clear_dir(cdx_dir.as_path())?;
        let bin = match env::var("CODEX_BIN") {
            Ok(value) if !value.trim().is_empty() => value,
            Ok(_) | Err(_) => String::from("codex"),
        };
        let log_max_bytes = match env::var("CDX_LOG_MAX_BYTES") {
            Ok(value) if !value.trim().is_empty() => value.parse::<usize>().map_err(|error| {
                format!("9d4b6c8e CDX_LOG_MAX_BYTES must be a positive integer: {error}")
            })?,
            Ok(_) | Err(_) => DEFAULT_LOG_MAX_BYTES,
        };
        if log_max_bytes == 0usize {
            return Err(String::from("0e5c7d9f CDX_LOG_MAX_BYTES must be greater than 0"));
        }
        let viewer = web::start_log_viewer(HARD_CODED_STATUS_SERVER_ADDRESS, cdx_dir.as_path())?;
        let progress_entries = Arc::new(Mutex::new(Vec::<ProgressEntry>::new()));
        let mut idx = 0usize;
        while let Some(task) = app_cfg.tasks.get(idx) {
            let task_started_at_unix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| {
                    format!("6d0f2a4c failed to compute task start unix time: {error}")
                })?
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
                    prompt: task.prompt.clone(),
                    task_started_at_unix,
                    total_repeat: task.repeat,
                });
            idx = idx.saturating_add(1usize);
        }
        let status_reporter = status::start_status_reporter(Arc::clone(&progress_entries));
        let mut init_idx = 0usize;
        while let Some(task) = app_cfg.tasks.get(init_idx) {
            let suffix = init_idx.saturating_add(1usize);
            let config_path = cdx_dir.join(format!(
                "{}_{}_cdx_cli.json",
                fs_ops::prompt_based_name(task.prompt.as_str()),
                suffix
            ));
            fs::write(config_path.as_path(), "{\"stop\": false}\n").map_err(|error| {
                format!(
                    "2f7b9c1d failed to create default stop config file `{}`: {error}",
                    config_path.display()
                )
            })?;
            init_idx = init_idx.saturating_add(1usize);
        }
        let run_result = runner::run_tasks(
            app_cfg.tasks.as_slice(),
            bin.as_str(),
            cdx_dir.as_path(),
            log_max_bytes,
            &progress_entries,
        );
        let status_result = status_reporter.stop();
        let viewer_result = viewer.stop();
        let mut errors = Vec::<String>::new();
        if let Err(error) = run_result {
            errors.push(error);
        }
        if let Err(error) = status_result {
            errors.push(error);
        }
        if let Err(error) = viewer_result {
            errors.push(error);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!("8f1b3d5a one or more execution stages failed: {}", errors.join(" | ")))
        }
    })() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            drop(stderr().write_all(format!("{error}\n").as_bytes()));
            ExitCode::FAILURE
        }
    }
}
