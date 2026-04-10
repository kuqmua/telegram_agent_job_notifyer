use std::{
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");

    let git_hash = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .and_then(|output| {
            if !output.status.success() {
                return None;
            }
            String::from_utf8(output.stdout)
                .ok()
                .map(|value| value.trim().to_owned())
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| String::from("unknown"));
    let build_time_utc = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .and_then(|output| {
            if !output.status.success() {
                return None;
            }
            String::from_utf8(output.stdout)
                .ok()
                .map(|value| value.trim().to_owned())
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            let fallback_epoch_seconds = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0u64, |duration| duration.as_secs());
            format!("unix:{fallback_epoch_seconds}")
        });

    println!("cargo:rustc-env=SERVER_GIT_HASH={git_hash}");
    println!("cargo:rustc-env=SERVER_BUILD_TIME_UTC={build_time_utc}");
}
