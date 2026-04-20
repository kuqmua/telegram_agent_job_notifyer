#![allow(clippy::single_call_fn)]

use std::{
    fs,
    fs::File,
    io::{ErrorKind, Read as _, Seek as _, SeekFrom},
    path::Path,
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;

#[derive(Deserialize)]
struct StopCfg {
    stop: bool,
}

pub(crate) fn clear_dir(cdx_dir: &Path) -> Result<(), String> {
    let dir_entries = fs::read_dir(cdx_dir).map_err(|error| {
        format!(
            "4e8a1c7d failed to read cdx_cli_manage directory `{}`: {error}",
            cdx_dir.display()
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
    Ok(())
}

pub(crate) fn prompt_based_name(prompt: &str) -> String {
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

pub(crate) fn read_stop_flag(config_path: &Path) -> Result<bool, String> {
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
        match serde_json::from_str::<StopCfg>(config_text.as_str()) {
            Ok(cfg) => return Ok(cfg.stop),
            Err(error) => {
                if started.elapsed().as_millis() < PARSE_RETRY_TOTAL_MS {
                    thread::sleep(Duration::from_millis(PARSE_RETRY_DELAY_MS));
                    continue;
                }
                return Err(format!(
                    "0d4f6a8b invalid `{}`: required boolean field `stop` is missing or \
                     malformed. Valid examples: {{\"stop\": false}} or {{\"stop\": true}}. \
                     Source: {error}",
                    config_path.display()
                ));
            }
        }
    }
}

pub(crate) fn trim_log_file(log_path: &Path, max_bytes: usize) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use std::{
        env::temp_dir,
        fs,
        path::PathBuf,
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{clear_dir, prompt_based_name, read_stop_flag, trim_log_file};

    fn unique_test_file(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0u128, |value| value.as_nanos());
        temp_dir().join(format!("cdx_cli_test_{}_{}_{}", process::id(), stamp, name))
    }

    #[test]
    fn prompt_based_name_replaces_forbidden_chars_and_spaces() {
        let result = prompt_based_name("a / b : c*?\"<>|");
        assert!(!result.is_empty());
        assert!(!result.contains(' '));
        assert!(!result.contains('/'));
        assert!(!result.contains(':'));
        assert!(!result.contains('*'));
        assert!(!result.contains('?'));
        assert!(!result.contains('"'));
        assert!(!result.contains('<'));
        assert!(!result.contains('>'));
        assert!(!result.contains('|'));
    }

    #[test]
    fn read_stop_flag_parses_true_and_false() {
        let path_true = unique_test_file("stop_true.json");
        let path_false = unique_test_file("stop_false.json");
        if let Err(error) = fs::write(path_true.as_path(), "{\"stop\": true}") {
            panic!("3b7d9f1a failed to write true stop file: {error}");
        }
        if let Err(error) = fs::write(path_false.as_path(), "{\"stop\": false}") {
            panic!("4c8e0a2b failed to write false stop file: {error}");
        }
        let stop_true = read_stop_flag(path_true.as_path());
        let stop_false = read_stop_flag(path_false.as_path());
        assert!(matches!(stop_true, Ok(true)));
        assert!(matches!(stop_false, Ok(false)));
        drop(fs::remove_file(path_true.as_path()));
        drop(fs::remove_file(path_false.as_path()));
    }

    #[test]
    fn read_stop_flag_rejects_missing_field() {
        let path = unique_test_file("stop_missing.json");
        if let Err(error) = fs::write(path.as_path(), "{\"flag\": true}") {
            panic!("5d9f1a3c failed to write invalid stop file: {error}");
        }
        let parsed = read_stop_flag(path.as_path());
        assert!(parsed.is_err());
        let message = parsed
            .err()
            .unwrap_or_else(|| String::from("missing error"));
        assert!(message.contains("required boolean field `stop`"));
        drop(fs::remove_file(path.as_path()));
    }

    #[test]
    fn clear_dir_removes_all_entries_inside_root() {
        let root_dir = unique_test_file("clear_dir_root");
        let nested_dir = root_dir.join("nested");
        let plain_file = root_dir.join("keep_me.tmp");
        let managed_log = root_dir.join("task_1_cdx_cli.log");
        let nested_file = nested_dir.join("nested.txt");
        fs::create_dir_all(nested_dir.as_path())
            .unwrap_or_else(|error| panic!("6e0a2b4d failed to create nested dir: {error}"));
        fs::write(plain_file.as_path(), "x")
            .unwrap_or_else(|error| panic!("7f1b3c5e failed to create plain file: {error}"));
        fs::write(managed_log.as_path(), "x")
            .unwrap_or_else(|error| panic!("8a2c4d6f failed to create managed file: {error}"));
        fs::write(nested_file.as_path(), "x")
            .unwrap_or_else(|error| panic!("9b3d5e7a failed to create nested file: {error}"));
        clear_dir(root_dir.as_path())
            .unwrap_or_else(|error| panic!("0c4e6f8a clear_dir failed unexpectedly: {error}"));
        let mut entries = fs::read_dir(root_dir.as_path()).unwrap_or_else(|error| {
            panic!("1d5f7a9b failed to read root dir after clear: {error}")
        });
        assert!(entries.next().is_none());
        drop(fs::remove_dir_all(root_dir.as_path()));
    }

    #[test]
    fn trim_log_file_keeps_full_line_when_cut_starts_on_line_boundary() {
        let log_path = unique_test_file("trim_boundary.log");
        fs::write(log_path.as_path(), "1111\n2222\n3333\n")
            .unwrap_or_else(|error| panic!("2d6f8a0c failed to write log fixture: {error}"));
        trim_log_file(log_path.as_path(), 10usize)
            .unwrap_or_else(|error| panic!("3e7a9c1d trim_log_file failed unexpectedly: {error}"));
        let trimmed = fs::read_to_string(log_path.as_path())
            .unwrap_or_else(|error| panic!("4f8b0d2e failed to read trimmed log: {error}"));
        assert_eq!(trimmed, "2222\n3333\n");
        drop(fs::remove_file(log_path.as_path()));
    }
}
