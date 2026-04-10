use std::{
    env::var_os,
    ffi::OsString,
    io::{self, Read as _, Write as _},
    process::{Command, Stdio},
    thread,
};
const DEFAULT_CAPTURE_MAXIMUM_BYTES: usize = 65_536;
pub fn exec_prompt(prompt: &str) -> io::Result<()> {
    drop(exec_prompt_capture(prompt)?);
    Ok(())
}
pub fn exec_prompt_capture(prompt: &str) -> io::Result<String> {
    exec_prompt_capture_limited(prompt, DEFAULT_CAPTURE_MAXIMUM_BYTES)
}
pub fn exec_prompt_capture_limited(
    prompt: &str,
    maximum_capture_bytes: usize,
) -> io::Result<String> {
    exec_prompt_capture_limited_with_binary(prompt, maximum_capture_bytes, None)
}
pub fn exec_prompt_capture_limited_with_binary(
    prompt: &str,
    maximum_capture_bytes: usize,
    configured_codex_binary_path: Option<&str>,
) -> io::Result<String> {
    let codex_binary = if let Some(binary_from_configuration) = configured_codex_binary_path {
        OsString::from(binary_from_configuration)
    } else if let Some(binary_from_environment) = var_os("CODEX_BIN") {
        binary_from_environment
    } else {
        let candidate_paths = ["codex", "codex-cli"];
        let mut resolved_path: Option<OsString> = None;
        for candidate_path in candidate_paths {
            let probe_status = Command::new(candidate_path)
                .arg("--version")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if probe_status.is_ok() {
                resolved_path = Some(OsString::from(candidate_path));
                break;
            }
        }
        resolved_path.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "codex binary not found. set CODEX_BINARY_PATH (or CODEX_BIN) to codex path",
            )
        })?
    };
    let authentication_output = Command::new(&codex_binary)
        .args(["login", "status"])
        .output()?;
    if !authentication_output.status.success() {
        drop(writeln!(
            io::stderr().lock(),
            "codex authentication check failed; run `codex login` and retry"
        ));
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "codex authentication check failed",
        ));
    }
    let mut child_process = Command::new(&codex_binary)
        .args(["exec", prompt])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout_pipe = child_process
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("failed to capture codex stdout stream"))?;
    let stderr_pipe = child_process
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("failed to capture codex stderr stream"))?;
    let stdout_reader_thread =
        thread::spawn(move || read_stream_with_limit(stdout_pipe, maximum_capture_bytes));
    let stderr_reader_thread =
        thread::spawn(move || read_stream_with_limit(stderr_pipe, maximum_capture_bytes));
    let child_exit_status = child_process.wait()?;
    let stdout_text = stdout_reader_thread
        .join()
        .map_err(|_join_error| io::Error::other("failed to join codex stdout reader thread"))??;
    let stderr_text = stderr_reader_thread
        .join()
        .map_err(|_join_error| io::Error::other("failed to join codex stderr reader thread"))??;
    if !child_exit_status.success() {
        return Err(io::Error::other(format!(
            "codex command failed with status {child_exit_status}: {stderr_text}"
        )));
    }
    if !stdout_text.trim().is_empty() {
        return Ok(stdout_text);
    }
    Ok(stderr_text)
}
fn read_stream_with_limit(
    mut input_stream: impl io::Read,
    maximum_capture_bytes: usize,
) -> io::Result<String> {
    let maximum_capture_bytes_with_sentinel = maximum_capture_bytes.saturating_add(1);
    let stream_read_limit =
        u64::try_from(maximum_capture_bytes_with_sentinel).map_err(|_conversion_error| {
            io::Error::other("maximum capture byte limit is too large for this platform")
        })?;
    let mut limited_reader = input_stream.by_ref().take(stream_read_limit);
    let mut captured_bytes = Vec::new();
    let _captured_byte_count = limited_reader.read_to_end(&mut captured_bytes)?;
    let is_truncated = if captured_bytes.len() > maximum_capture_bytes {
        captured_bytes.truncate(maximum_capture_bytes);
        true
    } else {
        false
    };
    let mut captured_text = String::from_utf8_lossy(&captured_bytes).into_owned();
    if is_truncated {
        captured_text.push_str("\n...[truncated by codex_cli]");
    }
    Ok(captured_text)
}
#[cfg(test)]
mod tests {
    use std::{
        env::temp_dir,
        fs, io,
        io::Write as _,
        path::{Path, PathBuf},
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::exec_prompt_capture_limited_with_binary;
    fn create_executable_script(script_name: &str, script_body: &str) -> io::Result<PathBuf> {
        let milliseconds_since_unix_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|clock_error| io::Error::other(format!("clock error: {clock_error}")))?
            .as_millis();
        let process_identifier = process::id();
        let temporary_directory = temp_dir();
        let mut created_script_path: Option<PathBuf> = None;
        let maximum_attempts = 64u32;
        for attempt_index in 0..maximum_attempts {
            let script_path = temporary_directory.join(format!(
                "codex_cli_{script_name}_{process_identifier}_{milliseconds_since_unix_epoch}_{attempt_index}.sh"
            ));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&script_path)
            {
                Ok(mut created_file) => {
                    created_file.write_all(script_body.as_bytes())?;
                    created_file.sync_all()?;
                    drop(created_file);
                    created_script_path = Some(script_path);
                    break;
                }
                Err(create_error) if create_error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(create_error) => return Err(create_error),
            }
        }
        let script_path = created_script_path.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "failed to create unique temporary script path",
            )
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mut script_permissions = fs::metadata(&script_path)?.permissions();
            script_permissions.set_mode(0o700);
            fs::set_permissions(&script_path, script_permissions)?;
        };
        Ok(script_path)
    }
    fn remove_script_file_if_exists(script_path: &Path) {
        let remove_result = fs::remove_file(script_path);
        if let Err(remove_file_error) = remove_result {
            let file_not_found_error_kind = io::ErrorKind::NotFound;
            assert_eq!(
                remove_file_error.kind(),
                file_not_found_error_kind,
                "failed to remove temporary script file: {remove_file_error}"
            );
        }
    }
    #[test]
    fn exec_prompt_capture_returns_stdout_text() {
        let script_path = create_executable_script(
            "stdout_success",
            r#"#!/usr/bin/env sh
if [ "$1" = "login" ] && [ "$2" = "status" ]; then
  exit 0
fi
if [ "$1" = "exec" ]; then
  printf "hello-from-stdout"
  exit 0
fi
exit 1
"#,
        )
        .expect("fc8b12d3");
        let script_path_text = script_path.to_string_lossy().into_owned();
        let result = exec_prompt_capture_limited_with_binary(
            "ignored prompt",
            1024,
            Some(&script_path_text),
        );
        remove_script_file_if_exists(&script_path);
        let captured_text = result.expect("6a2d3f94");
        assert_eq!(captured_text, "hello-from-stdout");
    }
    #[test]
    fn exec_prompt_capture_returns_stderr_text_when_stdout_is_empty() {
        let script_path = create_executable_script(
            "stderr_fallback",
            r#"#!/usr/bin/env sh
if [ "$1" = "login" ] && [ "$2" = "status" ]; then
  exit 0
fi
if [ "$1" = "exec" ]; then
  >&2 printf "hello-from-stderr"
  exit 0
fi
exit 1
"#,
        )
        .expect("4c9e17ab");
        let script_path_text = script_path.to_string_lossy().into_owned();
        let result = exec_prompt_capture_limited_with_binary(
            "ignored prompt",
            1024,
            Some(&script_path_text),
        );
        remove_script_file_if_exists(&script_path);
        let captured_text = result.expect("b7d4a8f1");
        assert_eq!(captured_text, "hello-from-stderr");
    }
    #[test]
    fn exec_prompt_capture_truncates_output_at_configured_limit() {
        let script_path = create_executable_script(
            "truncate_output",
            r#"#!/usr/bin/env sh
if [ "$1" = "login" ] && [ "$2" = "status" ]; then
  exit 0
fi
if [ "$1" = "exec" ]; then
  printf "abcdefghij"
  exit 0
fi
exit 1
"#,
        )
        .expect("91af2c6e");
        let script_path_text = script_path.to_string_lossy().into_owned();
        let result =
            exec_prompt_capture_limited_with_binary("ignored prompt", 5, Some(&script_path_text));
        remove_script_file_if_exists(&script_path);
        let captured_text = result.expect("3e5c7d9a");
        assert_eq!(captured_text, "abcde\n...[truncated by codex_cli]");
    }
}
