use std::{
    env::var_os,
    ffi::OsString,
    io::{self, Write as _},
    process::{Command, Stdio},
};

pub fn exec_prompt(prompt: &str) -> io::Result<()> {
    drop(exec_prompt_capture(prompt)?);
    Ok(())
}

pub fn exec_prompt_capture(prompt: &str) -> io::Result<String> {
    let codex_bin = resolve_codex_bin()?;
    check_auth(&codex_bin)?;
    let output = Command::new(&codex_bin).args(["exec", prompt]).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(io::Error::other(format!(
            "codex command failed with status {}: {}",
            output.status, stderr
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if !stdout.trim().is_empty() {
        return Ok(stdout);
    }
    Ok(String::from_utf8_lossy(&output.stderr).into_owned())
}

#[allow(
    clippy::single_call_fn,
    reason = "Helper extracted for readability and reuse in exported API path"
)]
fn check_auth(codex_bin: &OsString) -> io::Result<()> {
    let output = Command::new(codex_bin).args(["login", "status"]).output()?;
    if !output.status.success() {
        drop(writeln!(
            io::stderr().lock(),
            "codex authentication check failed; run `codex login` and retry"
        ));
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "codex authentication check failed",
        ));
    }
    Ok(())
}

#[allow(
    clippy::single_call_fn,
    reason = "Helper extracted for readability and reuse in exported API path"
)]
fn resolve_codex_bin() -> io::Result<OsString> {
    if let Some(bin) = var_os("CODEX_BIN") {
        return Ok(bin);
    }
    let mut found_bin = None;
    for candidate in [
        "codex",
        "codex-cli",
        "/home/kuqmua/.vscode/extensions/openai.chatgpt-26.325.31654-linux-x64/bin/linux-x86_64/\
         codex",
    ] {
        let probe = Command::new(candidate)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if probe.is_ok() {
            found_bin = Some(OsString::from(candidate));
            break;
        }
    }
    found_bin.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "codex binary not found. set CODEX_BIN=/path/to/codex (or codex-cli)",
        )
    })
}
