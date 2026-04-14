use std::{
    env::{split_paths, var_os},
    ffi::OsString,
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
    process::{self, Command, ExitStatus, Stdio},
    slice::Iter,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const DEFAULT_CAPTURE_MAXIMUM_BYTES: usize = 65_536;
const DEFAULT_ISOLATED_WORKSPACE_ROOT_DIRECTORY: &str = "/tmp/telegram_agent_codex_sandbox";
const ENVIRONMENT_VARIABLE_NAME_CODEX_HOME: &str = "CODEX_HOME";

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
pub struct AllowedEnvironmentVariableName(String);

impl AllowedEnvironmentVariableName {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for AllowedEnvironmentVariableName {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowedEnvironmentVariableNames(Vec<AllowedEnvironmentVariableName>);

impl AllowedEnvironmentVariableNames {
    pub fn iter(&self) -> Iter<'_, AllowedEnvironmentVariableName> {
        self.0.iter()
    }
}

impl From<Vec<String>> for AllowedEnvironmentVariableNames {
    fn from(value: Vec<String>) -> Self {
        Self(
            value
                .into_iter()
                .map(AllowedEnvironmentVariableName::from)
                .collect(),
        )
    }
}

impl<'allowed_environment_variable_names> IntoIterator
    for &'allowed_environment_variable_names AllowedEnvironmentVariableNames
{
    type IntoIter = Iter<'allowed_environment_variable_names, AllowedEnvironmentVariableName>;
    type Item = &'allowed_environment_variable_names AllowedEnvironmentVariableName;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxLauncherArgument(String);

impl SandboxLauncherArgument {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for SandboxLauncherArgument {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxLauncherArguments(Vec<SandboxLauncherArgument>);

impl SandboxLauncherArguments {
    pub fn iter(&self) -> Iter<'_, SandboxLauncherArgument> {
        self.0.iter()
    }
}

impl From<Vec<String>> for SandboxLauncherArguments {
    fn from(value: Vec<String>) -> Self {
        Self(
            value
                .into_iter()
                .map(SandboxLauncherArgument::from)
                .collect(),
        )
    }
}

impl<'sandbox_launcher_arguments> IntoIterator
    for &'sandbox_launcher_arguments SandboxLauncherArguments
{
    type IntoIter = Iter<'sandbox_launcher_arguments, SandboxLauncherArgument>;
    type Item = &'sandbox_launcher_arguments SandboxLauncherArgument;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxLauncherPath(String);

impl SandboxLauncherPath {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for SandboxLauncherPath {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxWorkspaceRoot(String);

impl SandboxWorkspaceRoot {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for SandboxWorkspaceRoot {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexExecutionIsolation {
    pub allow_network: bool,
    pub allowed_environment_variable_names: AllowedEnvironmentVariableNames,
    pub sandbox_auto_cleanup: bool,
    pub sandbox_enabled: bool,
    pub sandbox_launcher_arguments: SandboxLauncherArguments,
    pub sandbox_launcher_path: Option<SandboxLauncherPath>,
    pub sandbox_workspace_root: Option<SandboxWorkspaceRoot>,
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
    should_cleanup: bool,
}

impl Drop for EphemeralWorkspaceDirectoryGuard {
    fn drop(&mut self) {
        if !self.should_cleanup {
            return;
        }
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
    exec_prompt_capture_limited_with_binary_and_control_with_json_output(
        prompt,
        maximum_capture_bytes,
        configured_codex_binary_path,
        execution_timeout,
        cancellation_flag,
        execution_isolation,
        false,
    )
}

pub fn exec_prompt_capture_limited_with_binary_and_control_with_json_output(
    prompt: &str,
    maximum_capture_bytes: usize,
    configured_codex_binary_path: Option<&str>,
    execution_timeout: Option<Duration>,
    cancellation_flag: Option<&AtomicBool>,
    execution_isolation: Option<&CodexExecutionIsolation>,
    should_output_json_lines: bool,
) -> io::Result<PromptExecutionOutcome> {
    exec_prompt_capture_limited_with_binary_and_control_with_json_output_and_progress(
        prompt,
        maximum_capture_bytes,
        configured_codex_binary_path,
        execution_timeout,
        cancellation_flag,
        execution_isolation,
        should_output_json_lines,
        None,
    )
}

pub fn exec_prompt_capture_limited_with_binary_and_control_with_json_output_and_progress(
    prompt: &str,
    maximum_capture_bytes: usize,
    configured_codex_binary_path: Option<&str>,
    execution_timeout: Option<Duration>,
    cancellation_flag: Option<&AtomicBool>,
    execution_isolation: Option<&CodexExecutionIsolation>,
    should_output_json_lines: bool,
    progress_sender: Option<Sender<String>>,
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
                let resolved_candidate_path = var_os("PATH").and_then(|path_os| {
                    split_paths(&path_os)
                        .map(|path_item| path_item.join(candidate_path))
                        .find(|candidate_binary_path| candidate_binary_path.is_file())
                        .map(|candidate_binary_path| {
                            OsString::from(candidate_binary_path.as_os_str())
                        })
                });
                resolved_path =
                    Some(resolved_candidate_path.unwrap_or_else(|| OsString::from(candidate_path)));
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
    let ephemeral_workspace_guard =
        if let Some(isolation_configuration) = effective_execution_isolation {
            let workspace_root_directory = isolation_configuration
                .sandbox_workspace_root
                .as_ref()
                .map(SandboxWorkspaceRoot::as_str)
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
                should_cleanup: isolation_configuration.sandbox_auto_cleanup,
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

    let mut codex_execution_command = build_codex_command(
        &codex_binary,
        effective_execution_isolation,
        sandbox_workspace_directory,
    )?;
    let _codex_execution_subcommand = codex_execution_command.arg("exec");
    let _codex_execution_skip_git_repo_check = codex_execution_command.arg("--skip-git-repo-check");
    if should_output_json_lines {
        let _codex_execution_json_output = codex_execution_command.arg("--json");
    }
    let _codex_execution_prompt_argument = codex_execution_command.arg(prompt);
    let mut child_process = codex_execution_command
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
            progress_sender.as_ref(),
        )
    });
    let stderr_reader_thread = thread::spawn(move || {
        read_stream_with_limit_and_mirror(
            stderr_pipe,
            maximum_capture_bytes,
            ConsoleOutputTarget::StandardError,
            None,
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

    let stdout_text = stdout_reader_thread
        .join()
        .map_err(|_join_error| io::Error::other("failed to join codex stdout reader thread"))??;
    let stderr_text = stderr_reader_thread
        .join()
        .map_err(|_join_error| io::Error::other("failed to join codex stderr reader thread"))??;

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
        if let Some(sandbox_launcher_path) = isolation_configuration
            .sandbox_launcher_path
            .as_ref()
            .map(SandboxLauncherPath::as_str)
        {
            if sandbox_launcher_path.contains("bwrap") {
                let workspace_directory = sandbox_workspace_directory.ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "sandbox workspace directory is required for bwrap launcher",
                    )
                })?;
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
                let _sandbox_launcher_with_dev = sandbox_launcher_command.args(["--dev", "/dev"]);
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
                if let Ok(canonical_resolv_conf_path) = fs::canonicalize("/etc/resolv.conf") {
                    if let Some(resolv_conf_parent_directory) = canonical_resolv_conf_path.parent()
                    {
                        if resolv_conf_parent_directory.is_absolute()
                            && resolv_conf_parent_directory.exists()
                        {
                            let resolv_conf_parent_directory_text =
                                resolv_conf_parent_directory.to_str().ok_or_else(|| {
                                    io::Error::other(
                                        "resolver parent path is not valid UTF-8 for sandbox bind",
                                    )
                                })?;
                            let _sandbox_launcher_with_resolver_bind = sandbox_launcher_command
                                .args([
                                    "--ro-bind",
                                    resolv_conf_parent_directory_text,
                                    resolv_conf_parent_directory_text,
                                ]);
                        }
                    }
                }
                let codex_binary_path = PathBuf::from(codex_binary);
                if codex_binary_path.is_absolute() && codex_binary_path.exists() {
                    let canonical_codex_binary_path = fs::canonicalize(&codex_binary_path).ok();
                    let mut binary_bind_directories = Vec::<PathBuf>::new();
                    let mut push_bind_directory = |candidate_directory: &Path| {
                        if candidate_directory.is_absolute()
                            && candidate_directory.exists()
                            && !binary_bind_directories
                                .iter()
                                .any(|path| path == candidate_directory)
                        {
                            binary_bind_directories.push(PathBuf::from(candidate_directory));
                        }
                    };
                    for binary_path in [
                        Some(codex_binary_path.as_path()),
                        canonical_codex_binary_path.as_deref(),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        let mut maybe_directory = binary_path.parent();
                        let maximum_parent_levels_for_bind = 4usize;
                        for _ in 0..maximum_parent_levels_for_bind {
                            let Some(directory_path) = maybe_directory else {
                                break;
                            };
                            push_bind_directory(directory_path);
                            maybe_directory = directory_path.parent();
                        }
                    }
                    for bind_directory in binary_bind_directories {
                        let bind_directory_text = bind_directory.to_str().ok_or_else(|| {
                            io::Error::other(
                                "codex binary bind path is not valid UTF-8 for sandbox bind",
                            )
                        })?;
                        let _sandbox_launcher_with_binary_bind = sandbox_launcher_command.args([
                            "--ro-bind",
                            bind_directory_text,
                            bind_directory_text,
                        ]);
                    }
                }
                if let Some(codex_home_directory_os) = var_os(ENVIRONMENT_VARIABLE_NAME_CODEX_HOME)
                {
                    let codex_home_directory = PathBuf::from(codex_home_directory_os);
                    if codex_home_directory.is_absolute() && codex_home_directory.exists() {
                        let codex_home_directory_text =
                            codex_home_directory.to_str().ok_or_else(|| {
                                io::Error::other(
                                    "CODEX_HOME path is not valid UTF-8 for sandbox bind",
                                )
                            })?;
                        let _sandbox_launcher_with_codex_home_bind =
                            sandbox_launcher_command.args([
                                "--bind",
                                codex_home_directory_text,
                                codex_home_directory_text,
                            ]);
                    }
                }
                let workspace_directory_text = workspace_directory.to_str().ok_or_else(|| {
                    io::Error::other("workspace directory path is not valid UTF-8")
                })?;
                let _sandbox_launcher_with_workspace_bind = sandbox_launcher_command.args([
                    "--bind",
                    workspace_directory_text,
                    workspace_directory_text,
                ]);
                let _sandbox_launcher_with_change_directory =
                    sandbox_launcher_command.args(["--chdir", workspace_directory_text]);
                let _sandbox_launcher_with_home =
                    sandbox_launcher_command.args(["--setenv", "HOME", workspace_directory_text]);
                let _sandbox_launcher_with_tmpdir =
                    sandbox_launcher_command.args(["--setenv", "TMPDIR", workspace_directory_text]);
                let _sandbox_launcher_with_custom_arguments = sandbox_launcher_command.args(
                    isolation_configuration
                        .sandbox_launcher_arguments
                        .iter()
                        .map(SandboxLauncherArgument::as_str),
                );
                let _sandbox_launcher_with_binary = sandbox_launcher_command.arg(codex_binary);
                sandbox_launcher_command
            } else {
                let mut sandbox_launcher_command = Command::new(sandbox_launcher_path);
                let _sandbox_launcher_command_arguments = sandbox_launcher_command.args(
                    isolation_configuration
                        .sandbox_launcher_arguments
                        .iter()
                        .map(SandboxLauncherArgument::as_str),
                );
                let _sandbox_launcher_command_binary = sandbox_launcher_command.arg(codex_binary);
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
        for environment_variable_name in &isolation_configuration.allowed_environment_variable_names
        {
            if let Some(environment_variable_value) = var_os(environment_variable_name.as_str()) {
                let _command_with_allowed_environment =
                    command.env(environment_variable_name.as_str(), environment_variable_value);
            }
        }
        if let Some(workspace_directory) = sandbox_workspace_directory {
            let workspace_directory_text = workspace_directory
                .to_str()
                .ok_or_else(|| io::Error::other("workspace directory path is not valid UTF-8"))?;
            let _command_with_isolated_directories = command
                .current_dir(workspace_directory)
                .env("HOME", workspace_directory)
                .env("TMPDIR", workspace_directory_text);
        }
    }
    Ok(command)
}

fn read_stream_with_limit_and_mirror(
    mut input_stream: impl io::Read,
    maximum_capture_bytes: usize,
    console_output_target: ConsoleOutputTarget,
    progress_sender: Option<&Sender<String>>,
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
        if let Some(progress_sender_reference) = progress_sender {
            let progress_text = String::from_utf8_lossy(byte_chunk).into_owned();
            let _send_result = progress_sender_reference.send(progress_text);
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
        exec_prompt_capture_limited_with_binary_and_control_with_json_output,
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
    fn controlled_execution_can_request_json_output() {
        let script_path = create_executable_script(
            "json_output",
            r#"#!/usr/bin/env sh
if [ "$1" = "login" ] && [ "$2" = "status" ]; then
  exit 0
fi
	if [ "$1" = "exec" ] && [ "$2" = "--skip-git-repo-check" ] && [ "$3" = "--json" ] && [ "$4" = "ignored" ]; then
	  printf "{\"event\":\"task.started\"}"
	  exit 0
	fi
exit 1
"#,
        )
        .expect("c4b2a1d9");
        let script_path_text = script_path.to_string_lossy().into_owned();
        let maximum_attempts = 5u32;
        let last_attempt_index = 4u32;
        let mut execution_result = Err(io::Error::other("uninitialized retry result"));
        for attempt_index in 0..maximum_attempts {
            execution_result = exec_prompt_capture_limited_with_binary_and_control_with_json_output(
                "ignored",
                1024,
                Some(&script_path_text),
                None,
                None,
                None,
                true,
            );
            if execution_result
                .as_ref()
                .err()
                .is_some_and(|execution_error| {
                    execution_error.kind() == io::ErrorKind::ExecutableFileBusy
                })
                && attempt_index != last_attempt_index
            {
                thread::sleep(Duration::from_millis(25));
                continue;
            }
            break;
        }
        remove_script_file_if_exists(&script_path);
        let PromptExecutionOutcome::Completed(output_text) = execution_result.expect("f8e7d6c5")
        else {
            panic!("a2b3c4d5");
        };
        assert_eq!(output_text, "{\"event\":\"task.started\"}");
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
            allowed_environment_variable_names: vec![String::from("PATH")].into(),
            sandbox_auto_cleanup: true,
            sandbox_enabled: true,
            sandbox_launcher_arguments: Vec::<String>::new().into(),
            sandbox_launcher_path: None,
            sandbox_workspace_root: Some(workspace_root_path.to_string_lossy().into_owned().into()),
        };
        let maximum_attempts = 5u32;
        let last_attempt_index = 4u32;
        let mut execution_result = Err(io::Error::other("uninitialized retry result"));
        for attempt_index in 0..maximum_attempts {
            execution_result = exec_prompt_capture_limited_with_binary_and_control(
                "ignored",
                2048,
                Some(&script_path_text),
                None,
                None,
                Some(&execution_isolation),
            );
            if execution_result
                .as_ref()
                .err()
                .is_some_and(|execution_error| {
                    execution_error.kind() == io::ErrorKind::ExecutableFileBusy
                })
                && attempt_index != last_attempt_index
            {
                thread::sleep(Duration::from_millis(25));
                continue;
            }
            break;
        }
        let execution_outcome = execution_result.expect("a7b8c9d0");
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
    fn controlled_execution_uses_isolated_workspace_without_cleanup_when_disabled() {
        let script_path = create_executable_script(
            "isolated_workspace_no_cleanup",
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
        .expect("e1a2b3c4");
        let milliseconds_since_unix_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("f5a6b7c8")
            .as_millis();
        let workspace_root_path = temp_dir().join(format!(
            "codex_sandbox_root_no_cleanup_{}_{}",
            process::id(),
            milliseconds_since_unix_epoch
        ));
        fs::create_dir_all(&workspace_root_path).expect("d1e2f3a4");
        let script_path_text = script_path.to_string_lossy().into_owned();
        let execution_isolation = CodexExecutionIsolation {
            allow_network: false,
            allowed_environment_variable_names: vec![String::from("PATH")].into(),
            sandbox_auto_cleanup: false,
            sandbox_enabled: true,
            sandbox_launcher_arguments: Vec::<String>::new().into(),
            sandbox_launcher_path: None,
            sandbox_workspace_root: Some(workspace_root_path.to_string_lossy().into_owned().into()),
        };
        let maximum_attempts = 5u32;
        let last_attempt_index = 4u32;
        let mut execution_result = Err(io::Error::other("uninitialized retry result"));
        for attempt_index in 0..maximum_attempts {
            execution_result = exec_prompt_capture_limited_with_binary_and_control(
                "ignored",
                2048,
                Some(&script_path_text),
                None,
                None,
                Some(&execution_isolation),
            );
            if execution_result
                .as_ref()
                .err()
                .is_some_and(|execution_error| {
                    execution_error.kind() == io::ErrorKind::ExecutableFileBusy
                })
                && attempt_index != last_attempt_index
            {
                thread::sleep(Duration::from_millis(25));
                continue;
            }
            break;
        }
        let execution_outcome = execution_result.expect("a5b6c7d8");
        remove_script_file_if_exists(&script_path);
        let PromptExecutionOutcome::Completed(output_text) = execution_outcome else {
            panic!("b6c7d8e9");
        };
        let output_parts = output_text.split('|').collect::<Vec<_>>();
        let Some(current_working_directory) = output_parts.first() else {
            panic!("c7d8e9f0");
        };
        let workspace_root_text = workspace_root_path.to_string_lossy().into_owned();
        assert!(current_working_directory.starts_with(&workspace_root_text));
        let root_children_count = fs::read_dir(&workspace_root_path)
            .expect("d8e9f0a1")
            .count();
        assert!(root_children_count > 0);
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
            allowed_environment_variable_names: vec![String::from("PATH")].into(),
            sandbox_auto_cleanup: true,
            sandbox_enabled: true,
            sandbox_launcher_arguments: Vec::<String>::new().into(),
            sandbox_launcher_path: Some(String::from("/usr/bin/bwrap").into()),
            sandbox_workspace_root: Some(String::from("/tmp/unused").into()),
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
            allowed_environment_variable_names: vec![String::from("PATH")].into(),
            sandbox_auto_cleanup: true,
            sandbox_enabled: true,
            sandbox_launcher_arguments: Vec::<String>::new().into(),
            sandbox_launcher_path: Some(String::from("/usr/bin/bwrap").into()),
            sandbox_workspace_root: Some(String::from("/tmp/unused").into()),
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
