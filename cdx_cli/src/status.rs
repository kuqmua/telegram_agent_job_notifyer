#![allow(clippy::single_call_fn)]

use std::{
    io::{Write as _, stdout},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::types::ProgressEntry;

pub(crate) struct StatusReporter {
    handle: thread::JoinHandle<Result<(), String>>,
    stop: Arc<AtomicBool>,
}

impl StatusReporter {
    pub(crate) fn stop(self) -> Result<(), String> {
        self.stop.store(true, Ordering::Relaxed);
        self.handle
            .join()
            .map_err(|_error| String::from("5c9e1a3b minute status thread panicked"))?
    }
}
fn sanitize_status_prompt(prompt: &str) -> String {
    let mut out = String::with_capacity(prompt.len());
    for ch in prompt.chars() {
        if ch.is_ascii_control() {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    out
}

fn format_hms(total_secs: u64) -> String {
    let mut rest = total_secs;
    let mut hours = 0u64;
    while rest >= 3600u64 {
        rest = rest.saturating_sub(3600u64);
        hours = hours.saturating_add(1u64);
    }
    let mut minutes = 0u64;
    while rest >= 60u64 {
        rest = rest.saturating_sub(60u64);
        minutes = minutes.saturating_add(1u64);
    }
    let seconds = rest;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

pub(crate) fn start_status_reporter(
    progress_entries: Arc<Mutex<Vec<ProgressEntry>>>,
) -> StatusReporter {
    const STATUS_INTERVAL_SECS: u64 = 60u64;
    const STATUS_TICK_SECS: u64 = 1u64;
    let stop = Arc::new(AtomicBool::new(false));
    let stop_copy = Arc::clone(&stop);
    let handle = thread::spawn(move || -> Result<(), String> {
        let mut last_report_at = Instant::now();
        while !stop_copy.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_secs(STATUS_TICK_SECS));
            if stop_copy.load(Ordering::Relaxed) {
                break;
            }
            let now = Instant::now();
            if now.duration_since(last_report_at).as_secs() < STATUS_INTERVAL_SECS {
                continue;
            }
            last_report_at = now;
            let snapshot = match progress_entries.lock() {
                Ok(guard) => guard.clone(),
                Err(poisoned) => poisoned.into_inner().clone(),
            };
            let mut out = stdout().lock();
            let mut idx = 0usize;
            while let Some(entry) = snapshot.get(idx) {
                let state = if entry.finished {
                    "finished"
                } else if entry.current_iteration == 0u32 {
                    "idle"
                } else {
                    "running"
                };
                let prompt = sanitize_status_prompt(entry.prompt.as_str());
                let elapsed_secs = entry
                    .iteration_started_at
                    .map_or(0u64, |started_at| now.duration_since(started_at).as_secs());
                let now_unix_secs = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(entry.task_started_at_unix, |value| value.as_secs());
                let task_elapsed_secs = now_unix_secs.saturating_sub(entry.task_started_at_unix);
                let task_elapsed_hms = format_hms(task_elapsed_secs);
                let iteration_elapsed_hms = format_hms(elapsed_secs);
                writeln!(
                    out,
                    "minute_status: prompt=\"{}\" repeat={}/{} state={} task_start_unix={} \
                     task_elapsed={}s({}) iteration_elapsed={}s({})",
                    prompt,
                    entry.current_iteration,
                    entry.total_repeat,
                    state,
                    entry.task_started_at_unix,
                    task_elapsed_secs,
                    task_elapsed_hms,
                    elapsed_secs,
                    iteration_elapsed_hms
                )
                .map_err(|error| format!("9c3e5a7b failed to write minute status line: {error}"))?;
                idx = idx.saturating_add(1usize);
            }
        }
        Ok(())
    });
    StatusReporter { handle, stop }
}
#[cfg(test)]
mod tests {
    use super::{format_hms, sanitize_status_prompt};
    #[test]
    fn sanitize_status_prompt_replaces_control_chars() {
        let sanitized = sanitize_status_prompt("task\nname\t\r");
        assert_eq!(sanitized, "task_name__");
    }

    #[test]
    fn format_hms_renders_hours_minutes_seconds() {
        assert_eq!(format_hms(5u64), "00:00:05");
        assert_eq!(format_hms(65u64), "00:01:05");
        assert_eq!(format_hms(3661u64), "01:01:01");
    }
}
