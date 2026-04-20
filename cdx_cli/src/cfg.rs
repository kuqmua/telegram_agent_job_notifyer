#![allow(clippy::single_call_fn)]

use serde::Deserialize;
use serde_json::Value;

use crate::types::{AppCfg, TaskSpec};

#[derive(Deserialize)]
struct RawTaskSpec {
    prompt: String,
    repeat: u32,
}

fn validate_and_convert_tasks(raw_tasks: Vec<RawTaskSpec>) -> Result<Vec<TaskSpec>, String> {
    if raw_tasks.is_empty() {
        return Err(String::from("8c3a5b7d json tasks array must contain at least one object"));
    }
    let mut out = Vec::<TaskSpec>::with_capacity(raw_tasks.len());
    for raw_task in raw_tasks {
        if raw_task.repeat == 0u32 {
            return Err(String::from("6a1e3f5b `repeat` must be greater than 0"));
        }
        if raw_task.prompt.trim().is_empty() {
            return Err(String::from("7b2f4a6c `prompt` must be non-empty"));
        }
        out.push(TaskSpec {
            prompt: raw_task.prompt,
            repeat: raw_task.repeat,
        });
    }
    Ok(out)
}

pub(crate) fn parse_cfg(tasks_json: &str) -> Result<AppCfg, String> {
    let raw_value = serde_json::from_str::<Value>(tasks_json)
        .map_err(|error| format!("0f5a7b9d invalid tasks json: {error}"))?;
    if !raw_value.is_array() {
        return Err(String::from(
            "3f7a9c1d invalid format. Expected tasks array: [{\"prompt\":\"...\",\"repeat\":1}]",
        ));
    }
    let raw_tasks = serde_json::from_value::<Vec<RawTaskSpec>>(raw_value)
        .map_err(|error| format!("0f5a7b9d invalid tasks array: {error}"))?;
    Ok(AppCfg {
        tasks: validate_and_convert_tasks(raw_tasks)?,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_cfg;

    #[test]
    fn parse_cfg_accepts_array_format() {
        let input = r#"[{"prompt":"a","repeat":2}]"#;
        let cfg = parse_cfg(input)
            .unwrap_or_else(|error| panic!("1f5a7c9e parse failed unexpectedly: {error}"));
        assert_eq!(cfg.tasks.len(), 1usize);
        let first_task = cfg
            .tasks
            .first()
            .unwrap_or_else(|| panic!("2a6c8e1f expected first task to exist"));
        assert_eq!(first_task.prompt, "a");
        assert_eq!(first_task.repeat, 2u32);
    }

    #[test]
    fn parse_cfg_rejects_object_format() {
        let input = r#"{"server":"127.0.0.1:7878","tasks":[{"prompt":"a","repeat":1}]}"#;
        let message = parse_cfg(input)
            .err()
            .unwrap_or_else(|| String::from("missing error"));
        assert!(message.contains("invalid format. Expected tasks array"));
    }

    #[test]
    fn parse_cfg_rejects_zero_repeat() {
        let input = r#"[{"prompt":"a","repeat":0}]"#;
        let message = parse_cfg(input)
            .err()
            .unwrap_or_else(|| String::from("missing error"));
        assert!(message.contains("`repeat` must be greater than 0"));
    }

    #[test]
    fn parse_cfg_rejects_empty_prompt() {
        let input = r#"[{"prompt":"   ","repeat":1}]"#;
        let message = parse_cfg(input)
            .err()
            .unwrap_or_else(|| String::from("missing error"));
        assert!(message.contains("`prompt` must be non-empty"));
    }
}
