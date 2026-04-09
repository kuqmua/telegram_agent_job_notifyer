use std::{
    env::var_os,
    ffi::OsString,
    io::{self, Write as _},
    process::{Command, Stdio},
};

pub fn exec_prompt(prompt: &str) -> io::Result<()> {
    let codex_bin = {
        if let Some(bin) = var_os("CODEX_BIN") {
            bin
        } else {
            let mut found_bin = None;
            for candidate in [
                "codex",
                "codex-cli",
                "/home/kuqmua/.vscode/extensions/openai.chatgpt-26.325.31654-linux-x64/bin/\
                 linux-x86_64/codex",
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
            })?
        }
    };
    let output = Command::new(&codex_bin)
        .args(["login", "status"])
        .output()?;
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
    let status = Command::new(&codex_bin)
        .args(["exec", prompt])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    if !status.success() {
        return Err(io::Error::other(format!("codex command failed with status: {status}")));
    }
    Ok(())
}
