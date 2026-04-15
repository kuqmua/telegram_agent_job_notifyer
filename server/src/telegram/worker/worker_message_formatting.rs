use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::shared::{CodexTaskStatus, TaskSummary, VALUE_NONE};

pub(super) fn render_task_summary_message(
    task_summary: &TaskSummary,
    task_output: Option<&str>,
    queue_waiting: u64,
    running_now: u64,
) -> String {
    let current_unix_milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0u64, |duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX));
    let runtime_human = task_summary.started_unix_milliseconds.map_or_else(
        || {
            let queue_wait_milliseconds =
                current_unix_milliseconds.saturating_sub(task_summary.created_unix_milliseconds);
            let queue_wait_seconds = Duration::from_millis(queue_wait_milliseconds).as_secs_f64();
            format!("queued for {queue_wait_seconds:.1}s")
        },
        |started_unix_milliseconds| {
            let completed_or_now_unix_milliseconds = task_summary
                .finished_unix_milliseconds
                .unwrap_or(current_unix_milliseconds);
            let runtime_milliseconds =
                completed_or_now_unix_milliseconds.saturating_sub(started_unix_milliseconds);
            let runtime_seconds = Duration::from_millis(runtime_milliseconds).as_secs_f64();
            format!("{runtime_seconds:.1}s")
        },
    );
    let mut message_text = format!(
        "task_id={}\nstatus={}\ncreated_unix_milliseconds={}\nstarted_unix_milliseconds={}\\
         nfinished_unix_milliseconds={}\nqueue_waiting={}\nrunning_now={}\\
         nruntime={runtime_human}",
        task_summary.task_identifier,
        render_task_status(task_summary.status),
        task_summary.created_unix_milliseconds,
        task_summary
            .started_unix_milliseconds
            .map_or_else(|| String::from(VALUE_NONE), |value| value.to_string()),
        task_summary
            .finished_unix_milliseconds
            .map_or_else(|| String::from(VALUE_NONE), |value| value.to_string()),
        queue_waiting,
        running_now,
    );
    if let Some(task_output_text) = task_output {
        message_text.push_str("\noutput=\n");
        message_text.push_str(task_output_text);
    }
    message_text
}

pub(super) fn render_task_summaries(title: &str, task_summaries: &[TaskSummary]) -> String {
    let mut message_text = String::new();
    message_text.push_str(title);
    for task_summary in task_summaries {
        let task_line = format!(
            "\n- task_id={} status={} created={}",
            task_summary.task_identifier,
            render_task_status(task_summary.status),
            task_summary.created_unix_milliseconds,
        );
        message_text.push_str(&task_line);
    }
    message_text
}

const fn render_task_status(task_status: CodexTaskStatus) -> &'static str {
    match task_status {
        CodexTaskStatus::Cancelled => "cancelled",
        CodexTaskStatus::Failed => "failed",
        CodexTaskStatus::Queued => "queued",
        CodexTaskStatus::Running => "running",
        CodexTaskStatus::Succeeded => "succeeded",
        CodexTaskStatus::TimedOut => "timed_out",
    }
}
