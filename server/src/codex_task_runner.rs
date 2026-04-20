use std::{
    env, fs,
    fs::File,
    io::{ErrorKind, Read as _, Seek as _, SeekFrom, Write as _},
    num::NonZeroUsize,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex, PoisonError},
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;
use serde_json::Value;

pub const DEFAULT_LOG_MAXIMUM_BYTES: usize = 65_536usize;
pub const DEFAULT_MANAGED_DIRECTORY_NAME: &str = "cdx_cli_manage";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskRunnerConfiguration {
    pub codex_binary_path: String,
    pub log_maximum_bytes: usize,
    pub managed_directory_path: PathBuf,
}

#[derive(Deserialize)]
struct RawTaskSpecification {
    prompt: String,
    repeat: u32,
}

#[derive(Clone, Debug)]
struct TaskSpecification {
    prompt: String,
    repeat: u32,
}

#[derive(Clone, Debug)]
struct ProgressEntry {
    current_iteration: u32,
    finished: bool,
    iteration_started_at: Option<Instant>,
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

fn append_log_line(log_path: &Path, line: &str) -> Result<(), String> {
    let mut log_file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(log_path)
        .map_err(|error| {
            format!("0a2c4e6f failed to open log file for append `{}`: {error}", log_path.display())
        })?;
    writeln!(log_file, "{line}").map_err(|error| {
        format!("1b3d5f7a failed to append log line for `{}`: {error}", log_path.display())
    })
}

fn prompt_based_name(prompt: &str) -> String {
    let mut out = String::new();
    let mut out_len = 0usize;
    for ch in prompt.chars() {
        let mapped = if ch.is_ascii_control()
            || ch.is_ascii_whitespace()
            || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        {
            '_'
        } else {
            ch
        };
        out.push(mapped);
        out_len = out_len.saturating_add(1usize);
        if out_len >= 48usize {
            break;
        }
    }
    let trimmed = out.trim_matches('_').to_owned();
    if trimmed.is_empty() {
        String::from("task")
    } else {
        trimmed
    }
}

fn read_stop_flag(config_path: &Path) -> Result<bool, String> {
    const PARSE_RETRY_TOTAL_MS: u128 = 5000u128;
    const PARSE_RETRY_DELAY_MS: u64 = 50u64;
    let started = Instant::now();
    loop {
        let config_text = match fs::read_to_string(config_path) {
            Ok(value) => value,
            Err(error) => {
                let retryable = matches!(
                    error.kind(),
                    ErrorKind::NotFound
                        | ErrorKind::PermissionDenied
                        | ErrorKind::Interrupted
                        | ErrorKind::WouldBlock
                );
                if retryable && started.elapsed().as_millis() < PARSE_RETRY_TOTAL_MS {
                    thread::sleep(Duration::from_millis(PARSE_RETRY_DELAY_MS));
                    continue;
                }
                return Err(format!(
                    "9c3e5f7a required config file is missing: `{}` must exist in current working \
                     directory. Example content: {{\"stop\": false}}. Source: {error}",
                    config_path.display()
                ));
            }
        };
        match serde_json::from_str::<Value>(config_text.as_str()) {
            Ok(parsed) => {
                let stop = parsed.get("stop").and_then(Value::as_bool);
                if let Some(stop_value) = stop {
                    return Ok(stop_value);
                }
                if started.elapsed().as_millis() < PARSE_RETRY_TOTAL_MS {
                    thread::sleep(Duration::from_millis(PARSE_RETRY_DELAY_MS));
                    continue;
                }
                return Err(format!(
                    "0d4f6a8b invalid `{}`: required boolean field `stop` is missing or \
                     malformed. Valid examples: {{\"stop\": false}} or {{\"stop\": true}}.",
                    config_path.display()
                ));
            }
            Err(error) => {
                if started.elapsed().as_millis() < PARSE_RETRY_TOTAL_MS {
                    thread::sleep(Duration::from_millis(PARSE_RETRY_DELAY_MS));
                    continue;
                }
                return Err(format!(
                    "0d4f6a8b invalid `{}`: required boolean field `stop` is missing or \
                     malformed. Source: {error}",
                    config_path.display()
                ));
            }
        }
    }
}

fn trim_log_file(log_path: &Path, max_bytes: usize) -> Result<(), String> {
    let log_size_u64 = fs::metadata(log_path)
        .map_err(|error| {
            format!(
                "1e5f7a9c failed to read log metadata for trimming `{}`: {error}",
                log_path.display()
            )
        })?
        .len();
    let log_size = usize::try_from(log_size_u64).map_err(|error| {
        format!(
            "1e5f7a9c failed to convert log size for trimming `{}`: {error}",
            log_path.display()
        )
    })?;
    if log_size <= max_bytes {
        return Ok(());
    }
    let mut file = File::open(log_path).map_err(|error| {
        format!(
            "1e5f7a9c failed to open log file for trimming `{}`: {error}",
            log_path.display()
        )
    })?;
    let max_bytes_u64 = u64::try_from(max_bytes).map_err(|error| {
        format!(
            "1e5f7a9c failed to convert max bytes for trimming `{}`: {error}",
            log_path.display()
        )
    })?;
    let keep_from = log_size_u64.saturating_sub(max_bytes_u64);
    let starts_inside_line = if keep_from == 0u64 {
        false
    } else {
        let _seek_prev = file
            .seek(SeekFrom::Start(keep_from.saturating_sub(1u64)))
            .map_err(|error| {
                format!(
                    "1e5f7a9c failed to seek previous byte for trimming `{}`: {error}",
                    log_path.display()
                )
            })?;
        let mut prev_byte = [0u8; 1usize];
        file.read_exact(prev_byte.as_mut_slice()).map_err(|error| {
            format!(
                "1e5f7a9c failed to read previous byte for trimming `{}`: {error}",
                log_path.display()
            )
        })?;
        prev_byte[0usize] != b'\n'
    };
    let _seek_pos = file.seek(SeekFrom::Start(keep_from)).map_err(|error| {
        format!(
            "1e5f7a9c failed to seek log file for trimming `{}`: {error}",
            log_path.display()
        )
    })?;
    let mut tail = Vec::<u8>::with_capacity(max_bytes);
    let mut chunk = [0u8; 8192usize];
    loop {
        let read_count = file.read(&mut chunk).map_err(|error| {
            format!(
                "1e5f7a9c failed to read log tail for trimming `{}`: {error}",
                log_path.display()
            )
        })?;
        if read_count == 0usize {
            break;
        }
        let chunk_part = chunk.get(..read_count).ok_or_else(|| {
            format!("6f0a2b4c failed to split read chunk while trimming `{}`", log_path.display())
        })?;
        tail.extend_from_slice(chunk_part);
    }
    if starts_inside_line {
        let line_start = tail
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(0usize, |idx| idx.saturating_add(1usize));
        if line_start > 0usize {
            let _discarded = tail.drain(..line_start);
        }
    }
    fs::write(log_path, tail).map_err(|error| {
        format!("4b8d0f2a failed to rewrite trimmed log file `{}`: {error}", log_path.display())
    })?;
    Ok(())
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

    let raw_value = serde_json::from_str::<Value>(tasks_json)
        .map_err(|error| format!("0f5a7b9d invalid tasks json: {error}"))?;
    if !raw_value.is_array() {
        return Err(String::from(
            "3f7a9c1d invalid format. Expected tasks array: [{\"prompt\":\"...\",\"repeat\":1}]",
        ));
    }
    let raw_tasks = serde_json::from_value::<Vec<RawTaskSpecification>>(raw_value)
        .map_err(|error| format!("0f5a7b9d invalid tasks array: {error}"))?;
    if raw_tasks.is_empty() {
        return Err(String::from("8c3a5b7d json tasks array must contain at least one object"));
    }
    let mut task_specifications = Vec::<TaskSpecification>::with_capacity(raw_tasks.len());
    for raw_task in raw_tasks {
        if raw_task.repeat == 0u32 {
            return Err(String::from("6a1e3f5b `repeat` must be greater than 0"));
        }
        if raw_task.prompt.trim().is_empty() {
            return Err(String::from("7b2f4a6c `prompt` must be non-empty"));
        }
        task_specifications.push(TaskSpecification {
            prompt: raw_task.prompt,
            repeat: raw_task.repeat,
        });
    }

    let dir_entries = fs::read_dir(task_runner_configuration.managed_directory_path.as_path())
        .map_err(|error| {
            format!(
                "4e8a1c7d failed to read cdx_cli_manage directory `{}`: {error}",
                task_runner_configuration.managed_directory_path.display()
            )
        })?;
    for entry in dir_entries {
        let dir_entry =
            entry.map_err(|error| format!("5f9b2d8e failed to read directory entry: {error}"))?;
        let path = dir_entry.path();
        let file_type = dir_entry
            .file_type()
            .map_err(|error| format!("7b1d4f0a failed to read file type: {error}"))?;
        if file_type.is_dir() {
            fs::remove_dir_all(path.as_path()).map_err(|error| {
                format!(
                    "2c9e1a4b failed to cleanup managed subdirectory `{}`: {error}",
                    path.display()
                )
            })?;
            continue;
        }
        fs::remove_file(path.as_path()).map_err(|error| {
            format!("9d3f6b2c failed to remove managed file entry `{}`: {error}", path.display())
        })?;
    }

    let progress_entries = Arc::new(Mutex::new(Vec::<ProgressEntry>::new()));
    let mut task_index = 0usize;
    while task_specifications.get(task_index).is_some() {
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
    while let Some(task_specification) = task_specifications.get(stop_configuration_task_index) {
        let suffix = stop_configuration_task_index.saturating_add(1usize);
        let configuration_file_path =
            task_runner_configuration
                .managed_directory_path
                .join(format!(
                    "{}_{}_cdx_cli.json",
                    prompt_based_name(task_specification.prompt.as_str()),
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

    let max_parallel = thread::available_parallelism().map_or(1usize, NonZeroUsize::get);
    let mut errors = Vec::<String>::new();
    let mut batch_start = 0usize;
    while batch_start < task_specifications.len() {
        let batch_end = task_specifications
            .len()
            .min(batch_start.saturating_add(max_parallel));
        let mut handles = Vec::<thread::JoinHandle<Result<(), String>>>::new();
        let mut current_task_index = batch_start;
        while let Some(task_specification) = task_specifications.get(current_task_index) {
            if current_task_index >= batch_end {
                break;
            }
            let current_parallel_task_index = current_task_index;
            let codex_binary_path = task_runner_configuration.codex_binary_path.clone();
            let managed_directory_path = task_runner_configuration.managed_directory_path.clone();
            let progress_entries_copy = Arc::clone(&progress_entries);
            let task_copy = task_specification.clone();
            let log_maximum_bytes = task_runner_configuration.log_maximum_bytes;
            let handle = thread::spawn(move || -> Result<(), String> {
                let task_result = (|| -> Result<(), String> {
                    let suffix = current_parallel_task_index.saturating_add(1usize);
                    let prompt_name = prompt_based_name(task_copy.prompt.as_str());
                    let config_path =
                        managed_directory_path.join(format!("{prompt_name}_{suffix}_cdx_cli.json"));
                    let log_path =
                        managed_directory_path.join(format!("{prompt_name}_{suffix}_cdx_cli.log"));
                    let _initial_stop = read_stop_flag(config_path.as_path())?;
                    let total_started = Instant::now();
                    let mut iteration_index = 0u32;
                    while iteration_index < task_copy.repeat {
                        let stop_flag = read_stop_flag(config_path.as_path())?;
                        if stop_flag {
                            append_log_line(
                                log_path.as_path(),
                                format!(
                                    "graceful_stop: stop=true in `{}` after {iteration_index} \
                                     iteration(s)",
                                    config_path.display()
                                )
                                .as_str(),
                            )?;
                            break;
                        }
                        let iteration_number = iteration_index.saturating_add(1u32);
                        match progress_entries_copy.lock() {
                            Ok(mut guard) => guard,
                            Err(poisoned) => poisoned.into_inner(),
                        }
                        .get_mut(current_parallel_task_index)
                        .map(|progress_entry| {
                            progress_entry.current_iteration = iteration_number;
                            progress_entry.iteration_started_at = Some(Instant::now());
                        })
                        .ok_or_else(|| String::from("2f6b8d0e missing progress entry for task"))?;
                        append_log_line(
                            log_path.as_path(),
                            format!("iteration_start: {iteration_number}").as_str(),
                        )?;
                        let child_log = fs::OpenOptions::new()
                            .append(true)
                            .create(true)
                            .open(log_path.as_path())
                            .map_err(|error| {
                                format!(
                                    "4c9a1b3d failed to open log file for child stdio `{}`: \
                                     {error}",
                                    log_path.display()
                                )
                            })?;
                        let child_stdout = child_log.try_clone().map_err(|error| {
                            format!("5d0b2c4e failed to clone log file for stdout: {error}")
                        })?;
                        let child_stderr = child_log.try_clone().map_err(|error| {
                            format!("6e1c3d5f failed to clone log file for stderr: {error}")
                        })?;
                        let status = Command::new(codex_binary_path.as_str())
                            .args(["exec", task_copy.prompt.as_str()])
                            .stdin(Stdio::null())
                            .stderr(Stdio::from(child_stderr))
                            .stdout(Stdio::from(child_stdout))
                            .status()
                            .map_err(|error| {
                                format!(
                                    "7f2d4e6a failed to execute codex command `{} exec {}`; log \
                                     file: `{}`; source: {error}",
                                    codex_binary_path,
                                    task_copy.prompt,
                                    log_path.display()
                                )
                            })?;
                        drop(child_log);
                        trim_log_file(log_path.as_path(), log_maximum_bytes)?;
                        let code = status.code();
                        if !matches!(code, Some(0i32)) {
                            let log_bytes = match fs::read(log_path.as_path()) {
                                Ok(value) => value,
                                Err(error) => {
                                    return Err(format!(
                                        "2a4c6e8f failed to read log tail from `{}`: {error}",
                                        log_path.display()
                                    ));
                                }
                            };
                            let log_tail = if log_bytes.is_empty() {
                                String::from("log tail is empty")
                            } else {
                                let tail_slice = if log_bytes.len() > 2048usize {
                                    let skip = log_bytes.len().saturating_sub(2048usize);
                                    log_bytes
                                        .get(skip..)
                                        .map_or(log_bytes.as_slice(), |value| value)
                                } else {
                                    log_bytes.as_slice()
                                };
                                String::from_utf8_lossy(tail_slice).trim().to_owned()
                            };
                            let hint = if log_tail.contains(
                                "Not inside a trusted directory and --skip-git-repo-check was not \
                                 specified.",
                            ) {
                                "hint: run inside a trusted git directory or pass \
                                 `--skip-git-repo-check` to `codex exec`"
                            } else {
                                "hint: inspect the log tail below and full file path"
                            };
                            return Err(format!(
                                "8a3e5f7b command failed at iteration {} for `{} exec {}`, exit \
                                 code: {:?}; log file: `{}`; {}; log tail:\n{}",
                                iteration_index.saturating_add(1u32),
                                codex_binary_path,
                                task_copy.prompt,
                                code,
                                log_path.display(),
                                hint,
                                log_tail
                            ));
                        }
                        append_log_line(
                            log_path.as_path(),
                            format!("iteration_end: {iteration_number}").as_str(),
                        )?;
                        iteration_index = iteration_index.saturating_add(1u32);
                    }
                    let total_elapsed = total_started.elapsed();
                    let total_milliseconds = total_elapsed.as_millis();
                    append_log_line(
                        log_path.as_path(),
                        format!("total_execution_ms: {total_milliseconds}").as_str(),
                    )?;
                    let average_milliseconds = if iteration_index == 0u32 {
                        0u128
                    } else {
                        total_milliseconds
                            .checked_div(u128::from(iteration_index))
                            .ok_or_else(|| {
                                String::from("9b4f6a8c failed to compute average iteration time")
                            })?
                    };
                    append_log_line(
                        log_path.as_path(),
                        format!("average_iteration_ms: {average_milliseconds}").as_str(),
                    )?;
                    trim_log_file(log_path.as_path(), log_maximum_bytes)?;
                    Ok(())
                })();
                let finish_result = progress_entries_copy
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get_mut(current_parallel_task_index)
                    .map(|progress_entry| {
                        progress_entry.finished = true;
                        progress_entry.iteration_started_at = None;
                    })
                    .ok_or_else(|| String::from("4b8d0f2a missing progress entry for finish"));
                match (task_result, finish_result) {
                    (Ok(()), Ok(())) => Ok(()),
                    (Err(task_error), Ok(())) => Err(task_error),
                    (Ok(()), Err(finish_error)) => Err(finish_error),
                    (Err(task_error), Err(finish_error)) => Err(format!(
                        "6c8e0a2b task failed and finish update failed: {task_error} | \
                         {finish_error}"
                    )),
                }
            });
            handles.push(handle);
            current_task_index = current_task_index.saturating_add(1usize);
        }
        while let Some(handle) = handles.pop() {
            match handle.join() {
                Ok(inner) => {
                    if let Err(error) = inner {
                        errors.push(error);
                    }
                }
                Err(_error) => {
                    errors.push(String::from("1d6b8c0e parallel task thread panicked"));
                }
            }
        }
        batch_start = batch_end;
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("8f1b3d5a one or more execution stages failed: {}", errors.join(" | ")))
    }
}
