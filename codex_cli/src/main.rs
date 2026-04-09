use std::{
    env::{args_os, var_os},
    ffi::OsString,
    io::{self, Write as _},
    process::{Command, Stdio, exit},
};
fn main() {
    let args = args_os().skip(1).collect::<Vec<_>>();
    let codex_bin = var_os("CODEX_BIN").unwrap_or_else(|| {
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
        found_bin.unwrap_or_else(|| {
            drop(writeln!(
                io::stderr().lock(),
                "codex binary not found. set CODEX_BIN=/path/to/codex (or codex-cli)"
            ));
            exit(1i32);
        })
    });
    let mut forward_args = args;
    let codex_command = forward_args
        .first()
        .and_then(|arg| arg.to_str())
        .is_some_and(|first| {
            first.starts_with('-')
                || matches!(
                    first,
                    "exec"
                        | "e"
                        | "review"
                        | "login"
                        | "logout"
                        | "mcp"
                        | "mcp-server"
                        | "app-server"
                        | "completion"
                        | "sandbox"
                        | "debug"
                        | "apply"
                        | "a"
                        | "resume"
                        | "fork"
                        | "cloud"
                        | "features"
                        | "help"
                )
        });
    if !forward_args.is_empty() && !codex_command {
        let prompt = forward_args
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        forward_args = vec![OsString::from("exec"), OsString::from(prompt)];
    }
    let skip_auth_check = forward_args
        .first()
        .and_then(|arg| arg.to_str())
        .is_some_and(|first| {
            matches!(
                first,
                "login" | "logout" | "help" | "completion" | "--help" | "-h" | "--version" | "-V"
            )
        });
    if !skip_auth_check {
        let login_status = Command::new(&codex_bin).args(["login", "status"]).output();
        match login_status {
            Ok(output) => {
                if !output.status.success() {
                    drop(writeln!(
                        io::stderr().lock(),
                        "codex authentication check failed; run `codex login` and retry"
                    ));
                    exit(1i32);
                }
            }
            Err(error) => {
                drop(writeln!(
                    io::stderr().lock(),
                    "failed to run `{} login status`: {error}",
                    codex_bin.to_string_lossy()
                ));
                exit(1i32);
            }
        }
    }
    let status = Command::new(&codex_bin)
        .args(&forward_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();
    match status {
        Ok(status_code) => exit(status_code.code().map_or(1i32, |code| code)),
        Err(error) => {
            drop(writeln!(
                io::stderr().lock(),
                "failed to start {}: {error}",
                codex_bin.to_string_lossy()
            ));
            exit(1i32);
        }
    }
}
