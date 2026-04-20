#![allow(clippy::single_call_fn)]

use std::{
    fs,
    io::Write as _,
    num::NonZeroUsize,
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    thread,
    time::Instant,
};

use crate::{
    fs_ops::{prompt_based_name, read_stop_flag, trim_log_file},
    types::{ProgressEntry, TaskSpec},
};
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
fn read_log_tail_for_error(log_path: &Path, max_bytes: usize) -> String {
    let log_bytes = match fs::read(log_path) {
        Ok(value) => value,
        Err(error) => {
            return format!(
                "2a4c6e8f failed to read log tail from `{}`: {error}",
                log_path.display()
            );
        }
    };
    if log_bytes.is_empty() {
        return String::from("log tail is empty");
    }
    let tail_slice = if log_bytes.len() > max_bytes {
        let skip = log_bytes.len().saturating_sub(max_bytes);
        log_bytes
            .get(skip..)
            .map_or(log_bytes.as_slice(), |value| value)
    } else {
        log_bytes.as_slice()
    };
    String::from_utf8_lossy(tail_slice).trim().to_owned()
}

fn lock_progress_entries(
    progress_entries: &Arc<Mutex<Vec<ProgressEntry>>>,
) -> MutexGuard<'_, Vec<ProgressEntry>> {
    match progress_entries.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub(crate) fn run_tasks(
    tasks: &[TaskSpec],
    bin: &str,
    cdx_dir: &Path,
    log_max_bytes: usize,
    progress_entries: &Arc<Mutex<Vec<ProgressEntry>>>,
) -> Result<(), String> {
    let max_parallel = thread::available_parallelism().map_or(1usize, NonZeroUsize::get);
    let mut errors = Vec::<String>::new();
    let mut batch_start = 0usize;
    while batch_start < tasks.len() {
        let batch_end = tasks.len().min(batch_start.saturating_add(max_parallel));
        let mut handles = Vec::<thread::JoinHandle<Result<(), String>>>::new();
        let mut task_idx = batch_start;
        while let Some(task) = tasks.get(task_idx) {
            if task_idx >= batch_end {
                break;
            }
            let current_task_idx = task_idx;
            let bin_copy = bin.to_owned();
            let cdx_dir_copy = cdx_dir.to_path_buf();
            let progress_entries_copy = Arc::clone(progress_entries);
            let task_copy = task.clone();
            let handle = thread::spawn(move || -> Result<(), String> {
                let task_result = (|| -> Result<(), String> {
                    let suffix = current_task_idx.saturating_add(1usize);
                    let prompt_name = prompt_based_name(task_copy.prompt.as_str());
                    let config_path =
                        cdx_dir_copy.join(format!("{prompt_name}_{suffix}_cdx_cli.json"));
                    let log_path = cdx_dir_copy.join(format!("{prompt_name}_{suffix}_cdx_cli.log"));
                    let _initial_stop = read_stop_flag(config_path.as_path())?;
                    let total_started = Instant::now();
                    let mut iter_idx = 0u32;
                    while iter_idx < task_copy.repeat {
                        let stop_flag = read_stop_flag(config_path.as_path())?;
                        if stop_flag {
                            append_log_line(
                                log_path.as_path(),
                                format!(
                                    "graceful_stop: stop=true in `{}` after {iter_idx} \
                                     iteration(s)",
                                    config_path.display()
                                )
                                .as_str(),
                            )?;
                            break;
                        }
                        let iter_no = iter_idx.saturating_add(1u32);
                        lock_progress_entries(&progress_entries_copy)
                            .get_mut(current_task_idx)
                            .map(|progress_entry| {
                                progress_entry.current_iteration = iter_no;
                                progress_entry.iteration_started_at = Some(Instant::now());
                            })
                            .ok_or_else(|| {
                                String::from("2f6b8d0e missing progress entry for task")
                            })?;
                        append_log_line(
                            log_path.as_path(),
                            format!("iteration_start: {iter_no}").as_str(),
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
                        let status = Command::new(bin_copy.as_str())
                            .args(["exec", task_copy.prompt.as_str()])
                            .stdin(Stdio::null())
                            .stderr(Stdio::from(child_stderr))
                            .stdout(Stdio::from(child_stdout))
                            .status()
                            .map_err(|error| {
                                format!(
                                    "7f2d4e6a failed to execute codex command `{} exec {}`; log \
                                     file: `{}`; source: {error}",
                                    bin_copy,
                                    task_copy.prompt,
                                    log_path.display()
                                )
                            })?;
                        drop(child_log);
                        trim_log_file(log_path.as_path(), log_max_bytes)?;
                        let code = status.code();
                        if !matches!(code, Some(0i32)) {
                            let log_tail = read_log_tail_for_error(log_path.as_path(), 2048usize);
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
                                iter_idx.saturating_add(1u32),
                                bin_copy,
                                task_copy.prompt,
                                code,
                                log_path.display(),
                                hint,
                                log_tail
                            ));
                        }
                        append_log_line(
                            log_path.as_path(),
                            format!("iteration_end: {iter_no}").as_str(),
                        )?;
                        iter_idx = iter_idx.saturating_add(1u32);
                    }
                    let total_elapsed = total_started.elapsed();
                    let total_ms = total_elapsed.as_millis();
                    append_log_line(
                        log_path.as_path(),
                        format!("total_execution_ms: {total_ms}").as_str(),
                    )?;
                    let avg_ms = if iter_idx == 0u32 {
                        0u128
                    } else {
                        total_ms.checked_div(u128::from(iter_idx)).ok_or_else(|| {
                            String::from("9b4f6a8c failed to compute average iteration time")
                        })?
                    };
                    append_log_line(
                        log_path.as_path(),
                        format!("average_iteration_ms: {avg_ms}").as_str(),
                    )?;
                    trim_log_file(log_path.as_path(), log_max_bytes)?;
                    Ok(())
                })();
                let finish_result = progress_entries_copy
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get_mut(current_task_idx)
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
            task_idx = task_idx.saturating_add(1usize);
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
        Err(format!("8d2f4a6b one or more tasks failed: {}", errors.join(" | ")))
    }
}
