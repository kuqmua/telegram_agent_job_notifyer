mod codex_runtime {
    use std::{
        env::var_os,
        ffi::OsString,
        fs,
        io::{self, Write as _},
        path::{Path, PathBuf},
        process::{self, Command, ExitStatus, Stdio},
        sync::atomic::{AtomicBool, Ordering},
        thread,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    const DEFAULT_CAPTURE_MAXIMUM_BYTES: usize = 65_536;
    const DEFAULT_ISOLATED_WORKSPACE_ROOT_DIRECTORY: &str = "/tmp/telegram_agent_codex_sandbox";

    #[derive(Copy, Clone)]
    enum ConsoleOutputTarget {
        StandardError,
        StandardOutput,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum PromptExecutionOutcome {
        Cancelled,
        Completed(String),
        TimedOut,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct CodexExecutionIsolation {
        pub allow_network: bool,
        pub allowed_environment_variable_names: Vec<String>,
        pub sandbox_enabled: bool,
        pub sandbox_launcher_arguments: Vec<String>,
        pub sandbox_launcher_path: Option<String>,
        pub sandbox_workspace_root: Option<String>,
    }

    #[derive(Debug)]
    enum ChildWaitOutcome {
        Cancelled,
        Completed(ExitStatus),
        TimedOut,
    }

    #[derive(Debug)]
    struct EphemeralWorkspaceDirectoryGuard {
        path: PathBuf,
    }

    impl Drop for EphemeralWorkspaceDirectoryGuard {
        fn drop(&mut self) {
            let remove_result = fs::remove_dir_all(&self.path);
            if let Err(remove_error) = remove_result {
                if remove_error.kind() != io::ErrorKind::NotFound {
                    drop(writeln!(
                        io::stderr().lock(),
                        "failed to remove codex sandbox directory {}: {remove_error}",
                        self.path.display()
                    ));
                }
            }
        }
    }

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
        let execution_outcome = exec_prompt_capture_limited_with_binary_and_control(
            prompt,
            maximum_capture_bytes,
            configured_codex_binary_path,
            None,
            None,
            None,
        )?;
        match execution_outcome {
            PromptExecutionOutcome::Cancelled => Err(io::Error::other("codex execution cancelled")),
            PromptExecutionOutcome::Completed(output_text) => Ok(output_text),
            PromptExecutionOutcome::TimedOut => {
                Err(io::Error::new(io::ErrorKind::TimedOut, "codex execution timed out"))
            }
        }
    }

    pub fn exec_prompt_capture_limited_with_binary_and_control(
        prompt: &str,
        maximum_capture_bytes: usize,
        configured_codex_binary_path: Option<&str>,
        execution_timeout: Option<Duration>,
        cancellation_flag: Option<&AtomicBool>,
        execution_isolation: Option<&CodexExecutionIsolation>,
    ) -> io::Result<PromptExecutionOutcome> {
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

        let effective_execution_isolation = execution_isolation
            .filter(|isolation_configuration| isolation_configuration.sandbox_enabled);
        let ephemeral_workspace_guard = if let Some(isolation_configuration) =
            effective_execution_isolation
        {
            let workspace_root_directory = isolation_configuration
                .sandbox_workspace_root
                .as_deref()
                .map_or_else(
                    || PathBuf::from(DEFAULT_ISOLATED_WORKSPACE_ROOT_DIRECTORY),
                    PathBuf::from,
                );
            fs::create_dir_all(&workspace_root_directory)?;
            let process_identifier = process::id();
            let mut created_workspace_path: Option<PathBuf> = None;
            let maximum_attempts = 128u32;
            for attempt_index in 0..maximum_attempts {
                let milliseconds_since_unix_epoch = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|clock_error| io::Error::other(format!("clock error: {clock_error}")))?
                    .as_millis();
                let candidate_workspace_directory = workspace_root_directory.join(format!(
                    "job_{process_identifier}_{milliseconds_since_unix_epoch}_{attempt_index}"
                ));
                if candidate_workspace_directory.exists() {
                    continue;
                }
                fs::create_dir_all(&candidate_workspace_directory)?;
                created_workspace_path = Some(candidate_workspace_directory);
                break;
            }
            let workspace_path = created_workspace_path.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "failed to create unique sandbox workspace directory",
                )
            })?;
            Some(EphemeralWorkspaceDirectoryGuard {
                path: workspace_path,
            })
        } else {
            None
        };
        let sandbox_workspace_directory = ephemeral_workspace_guard
            .as_ref()
            .map(|workspace_guard| workspace_guard.path.as_path());
        let mut authentication_command = build_codex_command(
            &codex_binary,
            effective_execution_isolation,
            sandbox_workspace_directory,
        )?;
        let authentication_output = authentication_command
            .args(["login", "status"])
            .stdin(Stdio::null())
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

        let mut child_process = build_codex_command(
            &codex_binary,
            effective_execution_isolation,
            sandbox_workspace_directory,
        )?
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

        let stdout_reader_thread = thread::spawn(move || {
            read_stream_with_limit_and_mirror(
                stdout_pipe,
                maximum_capture_bytes,
                ConsoleOutputTarget::StandardOutput,
            )
        });
        let stderr_reader_thread = thread::spawn(move || {
            read_stream_with_limit_and_mirror(
                stderr_pipe,
                maximum_capture_bytes,
                ConsoleOutputTarget::StandardError,
            )
        });

        let child_wait_outcome = {
            let started_at = Instant::now();
            let poll_sleep_duration = Duration::from_millis(25);
            loop {
                if cancellation_flag.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                    let _kill_result = child_process.kill();
                    let _wait_result = child_process.wait();
                    break ChildWaitOutcome::Cancelled;
                }
                if execution_timeout
                    .is_some_and(|timeout_duration| started_at.elapsed() >= timeout_duration)
                {
                    let _kill_result = child_process.kill();
                    let _wait_result = child_process.wait();
                    break ChildWaitOutcome::TimedOut;
                }
                if let Some(exit_status) = child_process.try_wait()? {
                    break ChildWaitOutcome::Completed(exit_status);
                }
                thread::sleep(poll_sleep_duration);
            }
        };

        let stdout_text = stdout_reader_thread.join().map_err(|_join_error| {
            io::Error::other("failed to join codex stdout reader thread")
        })??;
        let stderr_text = stderr_reader_thread.join().map_err(|_join_error| {
            io::Error::other("failed to join codex stderr reader thread")
        })??;

        if matches!(child_wait_outcome, ChildWaitOutcome::Cancelled) {
            return Ok(PromptExecutionOutcome::Cancelled);
        }
        if matches!(child_wait_outcome, ChildWaitOutcome::TimedOut) {
            return Ok(PromptExecutionOutcome::TimedOut);
        }
        let ChildWaitOutcome::Completed(exit_status) = child_wait_outcome else {
            return Err(io::Error::other("unexpected child wait state"));
        };

        if !exit_status.success() {
            return Err(io::Error::other(format!(
                "codex command failed with status {exit_status}: {stderr_text}"
            )));
        }
        if !stdout_text.trim().is_empty() {
            return Ok(PromptExecutionOutcome::Completed(stdout_text));
        }
        Ok(PromptExecutionOutcome::Completed(stderr_text))
    }

    fn build_codex_command(
        codex_binary: &OsString,
        execution_isolation: Option<&CodexExecutionIsolation>,
        sandbox_workspace_directory: Option<&Path>,
    ) -> io::Result<Command> {
        let mut command = if let Some(isolation_configuration) = execution_isolation {
            if let Some(sandbox_launcher_path) =
                isolation_configuration.sandbox_launcher_path.as_deref()
            {
                if sandbox_launcher_path.contains("bwrap") {
                    let workspace_directory = sandbox_workspace_directory.ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "sandbox workspace directory is required for bwrap launcher",
                        )
                    })?;
                    let temporary_directory = workspace_directory.join("tmp");
                    fs::create_dir_all(&temporary_directory)?;
                    let mut sandbox_launcher_command = Command::new(sandbox_launcher_path);
                    let _sandbox_launcher_with_die_with_parent =
                        sandbox_launcher_command.arg("--die-with-parent");
                    let _sandbox_launcher_with_new_session =
                        sandbox_launcher_command.arg("--new-session");
                    let _sandbox_launcher_with_unshare_pid =
                        sandbox_launcher_command.arg("--unshare-pid");
                    let _sandbox_launcher_with_unshare_ipc =
                        sandbox_launcher_command.arg("--unshare-ipc");
                    let _sandbox_launcher_with_unshare_uts =
                        sandbox_launcher_command.arg("--unshare-uts");
                    if !isolation_configuration.allow_network {
                        let _sandbox_launcher_with_unshare_network =
                            sandbox_launcher_command.arg("--unshare-net");
                    }
                    let _sandbox_launcher_with_proc =
                        sandbox_launcher_command.args(["--proc", "/proc"]);
                    let _sandbox_launcher_with_dev =
                        sandbox_launcher_command.args(["--dev", "/dev"]);
                    for read_only_directory in [
                        "/usr",
                        "/usr/local",
                        "/bin",
                        "/sbin",
                        "/lib",
                        "/lib64",
                        "/etc",
                    ] {
                        if Path::new(read_only_directory).exists() {
                            let _sandbox_launcher_with_read_only_bind = sandbox_launcher_command
                                .args(["--ro-bind", read_only_directory, read_only_directory]);
                        }
                    }
                    let workspace_directory_text =
                        workspace_directory.to_str().ok_or_else(|| {
                            io::Error::other("workspace directory path is not valid UTF-8")
                        })?;
                    let temporary_directory_text =
                        temporary_directory.to_str().ok_or_else(|| {
                            io::Error::other("temporary directory path is not valid UTF-8")
                        })?;
                    let _sandbox_launcher_with_workspace_bind = sandbox_launcher_command.args([
                        "--bind",
                        workspace_directory_text,
                        workspace_directory_text,
                    ]);
                    let _sandbox_launcher_with_change_directory =
                        sandbox_launcher_command.args(["--chdir", workspace_directory_text]);
                    let _sandbox_launcher_with_home = sandbox_launcher_command.args([
                        "--setenv",
                        "HOME",
                        workspace_directory_text,
                    ]);
                    let _sandbox_launcher_with_tmpdir = sandbox_launcher_command.args([
                        "--setenv",
                        "TMPDIR",
                        temporary_directory_text,
                    ]);
                    let _sandbox_launcher_with_custom_arguments = sandbox_launcher_command
                        .args(&isolation_configuration.sandbox_launcher_arguments);
                    let _sandbox_launcher_with_binary = sandbox_launcher_command.arg(codex_binary);
                    sandbox_launcher_command
                } else {
                    let mut sandbox_launcher_command = Command::new(sandbox_launcher_path);
                    let _sandbox_launcher_command_arguments = sandbox_launcher_command
                        .args(&isolation_configuration.sandbox_launcher_arguments);
                    let _sandbox_launcher_command_binary =
                        sandbox_launcher_command.arg(codex_binary);
                    sandbox_launcher_command
                }
            } else {
                Command::new(codex_binary)
            }
        } else {
            Command::new(codex_binary)
        };
        if let Some(isolation_configuration) = execution_isolation {
            let _command_without_inherited_environment = command.env_clear();
            for environment_variable_name in
                &isolation_configuration.allowed_environment_variable_names
            {
                if let Some(environment_variable_value) = var_os(environment_variable_name) {
                    let _command_with_allowed_environment =
                        command.env(environment_variable_name, environment_variable_value);
                }
            }
            if let Some(workspace_directory) = sandbox_workspace_directory {
                let temporary_directory = workspace_directory.join("tmp");
                fs::create_dir_all(&temporary_directory)?;
                let _command_with_isolated_directories = command
                    .current_dir(workspace_directory)
                    .env("HOME", workspace_directory)
                    .env("TMPDIR", temporary_directory);
            }
        }
        Ok(command)
    }

    fn read_stream_with_limit_and_mirror(
        mut input_stream: impl io::Read,
        maximum_capture_bytes: usize,
        console_output_target: ConsoleOutputTarget,
    ) -> io::Result<String> {
        let maximum_capture_bytes_with_sentinel = maximum_capture_bytes.saturating_add(1usize);
        let mut captured_bytes = Vec::new();
        let mut temporary_buffer = [0u8; 4096];
        let mut is_truncated = false;
        loop {
            let bytes_read = input_stream.read(&mut temporary_buffer)?;
            if bytes_read == 0 {
                break;
            }
            let byte_chunk = temporary_buffer
                .get(..bytes_read)
                .ok_or_else(|| io::Error::other("failed to read chunk from codex stream buffer"))?;
            match console_output_target {
                ConsoleOutputTarget::StandardOutput => {
                    let mut standard_output = io::stdout().lock();
                    standard_output.write_all(byte_chunk)?;
                    standard_output.flush()?;
                }
                ConsoleOutputTarget::StandardError => {
                    let mut standard_error = io::stderr().lock();
                    standard_error.write_all(byte_chunk)?;
                    standard_error.flush()?;
                }
            }
            if captured_bytes.len() < maximum_capture_bytes_with_sentinel {
                let remaining_capacity =
                    maximum_capture_bytes_with_sentinel.saturating_sub(captured_bytes.len());
                let bytes_to_copy = remaining_capacity.min(byte_chunk.len());
                let captured_chunk = byte_chunk.get(..bytes_to_copy).ok_or_else(|| {
                    io::Error::other("failed to capture chunk from codex stream buffer")
                })?;
                captured_bytes.extend_from_slice(captured_chunk);
                if captured_bytes.len() == maximum_capture_bytes_with_sentinel {
                    is_truncated = true;
                    captured_bytes.truncate(maximum_capture_bytes);
                }
            } else {
                is_truncated = true;
            }
        }
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
            ffi::OsString,
            fs, io,
            io::Write as _,
            path::{Path, PathBuf},
            process,
            sync::{
                Arc,
                atomic::{AtomicBool, Ordering},
            },
            thread,
            time::{Duration, SystemTime, UNIX_EPOCH},
        };

        use super::{
            CodexExecutionIsolation, PromptExecutionOutcome, build_codex_command,
            exec_prompt_capture_limited_with_binary,
            exec_prompt_capture_limited_with_binary_and_control,
        };

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

        fn run_controlled_execution_with_retry(
            script_path_text: &str,
            execution_timeout: Option<Duration>,
            cancellation_flag: Option<&AtomicBool>,
        ) -> io::Result<PromptExecutionOutcome> {
            let maximum_attempts = 5u32;
            let last_attempt_index = 4u32;
            for attempt_index in 0..maximum_attempts {
                let execution_result = exec_prompt_capture_limited_with_binary_and_control(
                    "ignored",
                    1024,
                    Some(script_path_text),
                    execution_timeout,
                    cancellation_flag,
                    None,
                );
                match execution_result {
                    Ok(execution_outcome) => return Ok(execution_outcome),
                    Err(execution_error) => {
                        if execution_error.kind() == io::ErrorKind::ExecutableFileBusy
                            && attempt_index != last_attempt_index
                        {
                            thread::sleep(Duration::from_millis(25));
                            continue;
                        }
                        return Err(execution_error);
                    }
                }
            }
            Err(io::Error::other("unexpected retry loop exit"))
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
            let maximum_attempts = 5u32;
            let last_attempt_index = 4u32;
            let mut result = Err(io::Error::other("uninitialized retry result"));
            for attempt_index in 0..maximum_attempts {
                result = exec_prompt_capture_limited_with_binary(
                    "ignored prompt",
                    1024,
                    Some(&script_path_text),
                );
                if result.as_ref().err().is_some_and(|execution_error| {
                    execution_error.kind() == io::ErrorKind::ExecutableFileBusy
                }) && attempt_index != last_attempt_index
                {
                    thread::sleep(Duration::from_millis(25));
                    continue;
                }
                break;
            }
            remove_script_file_if_exists(&script_path);
            let captured_text = result.expect("6a2d3f94");
            assert_eq!(captured_text, "hello-from-stdout");
        }

        #[test]
        fn controlled_execution_returns_timeout() {
            let script_path = create_executable_script(
                "timeout",
                r#"#!/usr/bin/env sh
if [ "$1" = "login" ] && [ "$2" = "status" ]; then
  exit 0
fi
if [ "$1" = "exec" ]; then
  sleep 2
  echo "late"
  exit 0
fi
exit 1
"#,
            )
            .expect("3fbe28c1");
            let script_path_text = script_path.to_string_lossy().into_owned();
            let result = run_controlled_execution_with_retry(
                &script_path_text,
                Some(Duration::from_millis(100)),
                None,
            )
            .expect("1a7e4c39");
            remove_script_file_if_exists(&script_path);
            assert_eq!(result, PromptExecutionOutcome::TimedOut);
        }

        #[test]
        fn controlled_execution_returns_cancelled() {
            let script_path = create_executable_script(
                "cancel",
                r#"#!/usr/bin/env sh
if [ "$1" = "login" ] && [ "$2" = "status" ]; then
  exit 0
fi
if [ "$1" = "exec" ]; then
  sleep 2
  echo "late"
  exit 0
fi
exit 1
"#,
            )
            .expect("aed3f129");
            let script_path_text = script_path.to_string_lossy().into_owned();
            let cancellation_flag = Arc::new(AtomicBool::new(false));
            let cancellation_flag_for_thread = Arc::clone(&cancellation_flag);
            let cancellation_thread = thread::spawn(move || {
                thread::sleep(Duration::from_millis(100));
                cancellation_flag_for_thread.store(true, Ordering::Relaxed);
            });
            let result = run_controlled_execution_with_retry(
                &script_path_text,
                Some(Duration::from_secs(5)),
                Some(cancellation_flag.as_ref()),
            )
            .expect("bc1f7d45");
            cancellation_thread.join().expect("f2931c88");
            remove_script_file_if_exists(&script_path);
            assert_eq!(result, PromptExecutionOutcome::Cancelled);
        }

        #[test]
        fn controlled_execution_uses_isolated_workspace_and_cleans_it_up() {
            let script_path = create_executable_script(
                "isolated_workspace",
                r#"#!/usr/bin/env sh
if [ "$1" = "login" ] && [ "$2" = "status" ]; then
  exit 0
fi
if [ "$1" = "exec" ]; then
  printf "%s|%s|%s" "$PWD" "$HOME" "$TMPDIR"
  exit 0
fi
exit 1
"#,
            )
            .expect("a1b2c3d4");
            let milliseconds_since_unix_epoch = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("e5f6a7b8")
                .as_millis();
            let workspace_root_path = temp_dir().join(format!(
                "codex_sandbox_root_{}_{}",
                process::id(),
                milliseconds_since_unix_epoch
            ));
            fs::create_dir_all(&workspace_root_path).expect("c9d0e1f2");
            let script_path_text = script_path.to_string_lossy().into_owned();
            let execution_isolation = CodexExecutionIsolation {
                allow_network: false,
                allowed_environment_variable_names: vec![String::from("PATH")],
                sandbox_enabled: true,
                sandbox_launcher_arguments: Vec::new(),
                sandbox_launcher_path: None,
                sandbox_workspace_root: Some(workspace_root_path.to_string_lossy().into_owned()),
            };
            let execution_outcome = exec_prompt_capture_limited_with_binary_and_control(
                "ignored",
                2048,
                Some(&script_path_text),
                None,
                None,
                Some(&execution_isolation),
            )
            .expect("a7b8c9d0");
            remove_script_file_if_exists(&script_path);
            let PromptExecutionOutcome::Completed(output_text) = execution_outcome else {
                panic!("e4f5a6b7");
            };
            let output_parts = output_text.split('|').collect::<Vec<_>>();
            let Some(current_working_directory) = output_parts.first() else {
                panic!("b1c2d3e4");
            };
            let Some(home_directory) = output_parts.get(1) else {
                panic!("c5d6e7f8");
            };
            let Some(temporary_directory) = output_parts.get(2) else {
                panic!("d9e0f1a2");
            };
            let workspace_root_text = workspace_root_path.to_string_lossy().into_owned();
            assert!(current_working_directory.starts_with(&workspace_root_text));
            assert_eq!(current_working_directory, home_directory);
            assert!(temporary_directory.starts_with(current_working_directory));
            let root_children_count = fs::read_dir(&workspace_root_path)
                .expect("b4c5d6e7")
                .count();
            assert_eq!(root_children_count, 0);
            let remove_root_result = fs::remove_dir_all(&workspace_root_path);
            if let Err(remove_root_error) = remove_root_result {
                assert_eq!(remove_root_error.kind(), io::ErrorKind::NotFound);
            }
        }

        #[test]
        fn bwrap_command_contains_unshare_net_when_network_is_not_allowed() {
            let milliseconds_since_unix_epoch = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("a1b2c3d4")
                .as_millis();
            let workspace_directory = temp_dir().join(format!(
                "codex_sandbox_args_no_network_{}_{}",
                process::id(),
                milliseconds_since_unix_epoch
            ));
            fs::create_dir_all(&workspace_directory).expect("b2c3d4e5");
            let execution_isolation = CodexExecutionIsolation {
                allow_network: false,
                allowed_environment_variable_names: vec![String::from("PATH")],
                sandbox_enabled: true,
                sandbox_launcher_arguments: Vec::new(),
                sandbox_launcher_path: Some(String::from("/usr/bin/bwrap")),
                sandbox_workspace_root: Some(String::from("/tmp/unused")),
            };
            let codex_binary = OsString::from("/usr/local/bin/codex");
            let command = build_codex_command(
                &codex_binary,
                Some(&execution_isolation),
                Some(workspace_directory.as_path()),
            )
            .expect("c3d4e5f6");
            let command_arguments = command
                .get_args()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            assert!(
                command_arguments
                    .iter()
                    .any(|argument| argument == "--unshare-net")
            );
            let remove_workspace_result = fs::remove_dir_all(&workspace_directory);
            if let Err(remove_workspace_error) = remove_workspace_result {
                assert_eq!(remove_workspace_error.kind(), io::ErrorKind::NotFound);
            }
        }

        #[test]
        fn bwrap_command_does_not_contain_unshare_net_when_network_is_allowed() {
            let milliseconds_since_unix_epoch = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("d4e5f6a7")
                .as_millis();
            let workspace_directory = temp_dir().join(format!(
                "codex_sandbox_args_with_network_{}_{}",
                process::id(),
                milliseconds_since_unix_epoch
            ));
            fs::create_dir_all(&workspace_directory).expect("e5f6a7b8");
            let execution_isolation = CodexExecutionIsolation {
                allow_network: true,
                allowed_environment_variable_names: vec![String::from("PATH")],
                sandbox_enabled: true,
                sandbox_launcher_arguments: Vec::new(),
                sandbox_launcher_path: Some(String::from("/usr/bin/bwrap")),
                sandbox_workspace_root: Some(String::from("/tmp/unused")),
            };
            let codex_binary = OsString::from("/usr/local/bin/codex");
            let command = build_codex_command(
                &codex_binary,
                Some(&execution_isolation),
                Some(workspace_directory.as_path()),
            )
            .expect("f6a7b8c9");
            let command_arguments = command
                .get_args()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            assert!(
                !command_arguments
                    .iter()
                    .any(|argument| argument == "--unshare-net")
            );
            let remove_workspace_result = fs::remove_dir_all(&workspace_directory);
            if let Err(remove_workspace_error) = remove_workspace_result {
                assert_eq!(remove_workspace_error.kind(), io::ErrorKind::NotFound);
            }
        }
    }
}

pub use codex_runtime::{
    CodexExecutionIsolation, PromptExecutionOutcome, exec_prompt, exec_prompt_capture,
    exec_prompt_capture_limited, exec_prompt_capture_limited_with_binary,
    exec_prompt_capture_limited_with_binary_and_control,
};
use serde::{Deserialize, Serialize};

pub const SYSTEM_MESSAGE_PREFIX: &str = "[telegram-agent]";
pub const SYSTEM_MESSAGE_HEALTHY: &str = "Health check: bot is alive";
pub const SYSTEM_MESSAGE_HELP: &str =
    "Commands:\n/health - bot health\n/help - this help\n/codex <prompt> - create task\n/status \
     <task_id> - task details\n/list - recent tasks\n/active - active tasks\n/cancel <task_id> - \
     cancel task\n/retry <task_id> - retry task\n/output <task_id> - task output only\n/last - \
     latest task\n/queue - queue status\n/stats - task counters\n/limits - runtime \
     limits\n/whoami - sender identity\n/version - build info\n\nExamples:\n/codex explain \
     ownership in rust\n/status 42\n/output 42\n/retry 42";
pub const SYSTEM_MESSAGE_CODEX_USAGE: &str = "Usage: /codex <prompt>";
pub const SYSTEM_MESSAGE_CODEX_STARTED: &str = "Task started";
pub const SYSTEM_MESSAGE_CODEX_QUEUED: &str = "Task queued";
pub const SYSTEM_MESSAGE_CODEX_FINISHED: &str = "Task finished";
pub const SYSTEM_MESSAGE_CODEX_BUSY: &str = "Task is still running, please wait";
pub const SYSTEM_MESSAGE_CODEX_CANCELLED: &str = "Task cancelled";
pub const SYSTEM_MESSAGE_CODEX_TIMED_OUT: &str = "Task timed out";
pub const SYSTEM_MESSAGE_UNKNOWN_COMMAND: &str = "Unknown command";
pub const SYSTEM_MESSAGE_INVALID_COMMAND_ARGUMENTS: &str = "Invalid command arguments";
pub const SYSTEM_MESSAGE_TASK_NOT_FOUND: &str = "Task not found";
pub const SYSTEM_MESSAGE_TASK_ACCESS_DENIED: &str = "Task access denied";
pub const SYSTEM_MESSAGE_TASK_RATE_LIMITED: &str = "Task rate limit exceeded";
pub const SYSTEM_MESSAGE_TASK_PROMPT_TOO_LONG: &str = "Prompt too long";
pub const SYSTEM_MESSAGE_TASK_QUEUE_WAIT_EXCEEDED: &str =
    "Task cancelled: queue wait limit exceeded";
pub const SYSTEM_MESSAGE_USERNAME_REQUIRED: &str = "username required";
pub const SYSTEM_MESSAGE_EMPTY_CODEX_OUTPUT: &str = "(empty codex output)";
pub const SYSTEM_MESSAGE_TRUNCATED_SUFFIX: &str = "\n...[truncated]";
pub const SYSTEM_MESSAGES_ALL: [&str; 20] = [
    SYSTEM_MESSAGE_PREFIX,
    SYSTEM_MESSAGE_HEALTHY,
    SYSTEM_MESSAGE_HELP,
    SYSTEM_MESSAGE_CODEX_USAGE,
    SYSTEM_MESSAGE_CODEX_STARTED,
    SYSTEM_MESSAGE_CODEX_QUEUED,
    SYSTEM_MESSAGE_CODEX_FINISHED,
    SYSTEM_MESSAGE_CODEX_BUSY,
    SYSTEM_MESSAGE_CODEX_CANCELLED,
    SYSTEM_MESSAGE_CODEX_TIMED_OUT,
    SYSTEM_MESSAGE_UNKNOWN_COMMAND,
    SYSTEM_MESSAGE_INVALID_COMMAND_ARGUMENTS,
    SYSTEM_MESSAGE_TASK_NOT_FOUND,
    SYSTEM_MESSAGE_TASK_ACCESS_DENIED,
    SYSTEM_MESSAGE_TASK_RATE_LIMITED,
    SYSTEM_MESSAGE_TASK_PROMPT_TOO_LONG,
    SYSTEM_MESSAGE_TASK_QUEUE_WAIT_EXCEEDED,
    SYSTEM_MESSAGE_USERNAME_REQUIRED,
    SYSTEM_MESSAGE_EMPTY_CODEX_OUTPUT,
    SYSTEM_MESSAGE_TRUNCATED_SUFFIX,
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncomingCommand {
    Active,
    Cancel(u64),
    Codex(String),
    Health,
    Help,
    Invalid {
        command_name: &'static str,
        message: String,
    },
    Last,
    Limits,
    List,
    Output(u64),
    Queue,
    Retry(u64),
    Stats,
    Status(u64),
    Unknown,
    Version,
    WhoAmI,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodexTaskStatus {
    Cancelled,
    Failed,
    Queued,
    Running,
    Succeeded,
    TimedOut,
}

impl CodexTaskStatus {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Cancelled | Self::Failed | Self::Succeeded | Self::TimedOut)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskOwner {
    pub chat_identifier: i64,
    pub sender_username: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCreationRequest {
    pub owner: TaskOwner,
    pub prompt_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSummary {
    pub created_unix_milliseconds: u64,
    pub finished_unix_milliseconds: Option<u64>,
    pub owner: TaskOwner,
    pub started_unix_milliseconds: Option<u64>,
    pub status: CodexTaskStatus,
    pub task_identifier: u64,
}

#[must_use]
pub fn parse_incoming_command(input_text: &str) -> IncomingCommand {
    let trimmed_input_text = input_text.trim();
    if trimmed_input_text.eq_ignore_ascii_case("/health") {
        return IncomingCommand::Health;
    }
    if trimmed_input_text.eq_ignore_ascii_case("/help") {
        return IncomingCommand::Help;
    }
    if trimmed_input_text.eq_ignore_ascii_case("/list") {
        return IncomingCommand::List;
    }
    if trimmed_input_text.eq_ignore_ascii_case("/last") {
        return IncomingCommand::Last;
    }
    if trimmed_input_text.eq_ignore_ascii_case("/active") {
        return IncomingCommand::Active;
    }
    if trimmed_input_text.eq_ignore_ascii_case("/queue") {
        return IncomingCommand::Queue;
    }
    if trimmed_input_text.eq_ignore_ascii_case("/stats") {
        return IncomingCommand::Stats;
    }
    if trimmed_input_text.eq_ignore_ascii_case("/limits") {
        return IncomingCommand::Limits;
    }
    if trimmed_input_text.eq_ignore_ascii_case("/whoami") {
        return IncomingCommand::WhoAmI;
    }
    if trimmed_input_text.eq_ignore_ascii_case("/version") {
        return IncomingCommand::Version;
    }
    if let Some(raw_prompt) = trimmed_input_text.strip_prefix("/codex") {
        return IncomingCommand::Codex(raw_prompt.trim().to_owned());
    }
    if let Some(command_arguments) = trimmed_input_text.strip_prefix("/status") {
        return parse_u64_command_argument("status", command_arguments)
            .map_or_else(|invalid_message| invalid_message, IncomingCommand::Status);
    }
    if let Some(command_arguments) = trimmed_input_text.strip_prefix("/cancel") {
        return parse_u64_command_argument("cancel", command_arguments)
            .map_or_else(|invalid_message| invalid_message, IncomingCommand::Cancel);
    }
    if let Some(command_arguments) = trimmed_input_text.strip_prefix("/retry") {
        return parse_u64_command_argument("retry", command_arguments)
            .map_or_else(|invalid_message| invalid_message, IncomingCommand::Retry);
    }
    if let Some(command_arguments) = trimmed_input_text.strip_prefix("/output") {
        return parse_u64_command_argument("output", command_arguments)
            .map_or_else(|invalid_message| invalid_message, IncomingCommand::Output);
    }
    IncomingCommand::Unknown
}

#[must_use]
pub fn format_system_message(message_text: &str) -> String {
    format!("{SYSTEM_MESSAGE_PREFIX} {message_text}")
}

#[must_use]
pub fn normalize_codex_output(raw_output: &str, maximum_characters: usize) -> String {
    let trimmed_output = raw_output.trim();
    if trimmed_output.is_empty() {
        return String::from(SYSTEM_MESSAGE_EMPTY_CODEX_OUTPUT);
    }
    let output_character_count = trimmed_output.chars().count();
    if output_character_count <= maximum_characters {
        return trimmed_output.to_owned();
    }
    let truncated_output = trimmed_output
        .chars()
        .take(maximum_characters)
        .collect::<String>();
    format!("{truncated_output}{SYSTEM_MESSAGE_TRUNCATED_SUFFIX}")
}

#[must_use]
pub fn split_text_into_chunks(
    message_text: &str,
    maximum_characters_per_chunk: usize,
) -> Vec<String> {
    if message_text.is_empty() {
        return vec![String::new()];
    }
    let mut result_chunks = Vec::new();
    let mut current_chunk = String::new();
    let mut current_chunk_character_count = 0usize;
    for current_character in message_text.chars() {
        if current_chunk_character_count >= maximum_characters_per_chunk {
            result_chunks.push(current_chunk);
            current_chunk = String::new();
            current_chunk_character_count = 0;
        }
        current_chunk.push(current_character);
        current_chunk_character_count = current_chunk_character_count.saturating_add(1);
    }
    if !current_chunk.is_empty() {
        result_chunks.push(current_chunk);
    }
    result_chunks
}

fn parse_u64_command_argument(
    command_name: &'static str,
    command_arguments: &str,
) -> Result<u64, IncomingCommand> {
    let trimmed_arguments = command_arguments.trim();
    if trimmed_arguments.is_empty() {
        return Err(IncomingCommand::Invalid {
            command_name,
            message: String::from("task identifier is required"),
        });
    }
    match trimmed_arguments.parse::<u64>() {
        Ok(task_identifier) => Ok(task_identifier),
        Err(parse_error) => Err(IncomingCommand::Invalid {
            command_name,
            message: format!("task identifier must be u64: {parse_error}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CodexTaskStatus, IncomingCommand, SYSTEM_MESSAGES_ALL, normalize_codex_output,
        parse_incoming_command, split_text_into_chunks,
    };

    #[test]
    fn parse_command_health() {
        assert_eq!(parse_incoming_command(" /health "), IncomingCommand::Health);
    }

    #[test]
    fn parse_command_codex() {
        assert_eq!(
            parse_incoming_command("/codex  explain rust ownership"),
            IncomingCommand::Codex(String::from("explain rust ownership"))
        );
    }

    #[test]
    fn parse_command_status() {
        assert_eq!(parse_incoming_command("/status 42"), IncomingCommand::Status(42));
    }

    #[test]
    fn parse_command_whoami() {
        assert_eq!(parse_incoming_command("/whoami"), IncomingCommand::WhoAmI);
    }

    #[test]
    fn parse_command_version() {
        assert_eq!(parse_incoming_command("/version"), IncomingCommand::Version);
    }

    #[test]
    fn parse_command_output() {
        assert_eq!(parse_incoming_command("/output 42"), IncomingCommand::Output(42));
    }

    #[test]
    fn parse_command_queue() {
        assert_eq!(parse_incoming_command("/queue"), IncomingCommand::Queue);
    }

    #[test]
    fn parse_command_stats() {
        assert_eq!(parse_incoming_command("/stats"), IncomingCommand::Stats);
    }

    #[test]
    fn parse_command_invalid_cancel() {
        let parsed_command = parse_incoming_command("/cancel abc");
        assert!(matches!(parsed_command, IncomingCommand::Invalid {
            command_name: "cancel",
            ..
        }));
    }

    #[test]
    fn parse_command_unknown() {
        assert_eq!(parse_incoming_command("/unknown"), IncomingCommand::Unknown);
    }

    #[test]
    fn normalize_output_empty() {
        assert_eq!(normalize_codex_output("   ", 10), "(empty codex output)");
    }

    #[test]
    fn normalize_output_truncated() {
        let normalized_output = normalize_codex_output("abcdef", 3);
        assert_eq!(normalized_output, "abc\n...[truncated]");
    }

    #[test]
    fn split_text_chunks() {
        let result_chunks = split_text_into_chunks("abcdef", 2);
        assert_eq!(result_chunks, vec!["ab", "cd", "ef"]);
    }

    #[test]
    fn codex_task_status_terminal() {
        assert!(CodexTaskStatus::Succeeded.is_terminal());
        assert!(!CodexTaskStatus::Running.is_terminal());
    }

    #[test]
    fn system_messages_use_ascii_symbols_only() {
        for system_message in SYSTEM_MESSAGES_ALL {
            assert!(
                system_message.is_ascii(),
                "system message contains non-ASCII symbols: {system_message}"
            );
        }
    }
}
