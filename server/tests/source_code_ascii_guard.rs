use axum as _;
use dotenvy as _;
use reqwest as _;
use serde as _;
use serde_json as _;
use server as _;
use telegram_agent_shared as _;
use thiserror as _;
use tokio as _;
use tracing as _;
use tracing_subscriber as _;

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command,
    };

    #[test]
    fn all_tracked_files_use_ascii_symbols_only() {
        let workspace_root_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map_or_else(PathBuf::new, PathBuf::from);
        assert!(
            !workspace_root_path.as_os_str().is_empty(),
            "f1a2b3c4: failed to resolve workspace root path"
        );
        let command_output = Command::new("git")
            .arg("ls-files")
            .arg("-z")
            .current_dir(&workspace_root_path)
            .output()
            .expect("d3a9f1c7");
        assert!(
            command_output.status.success(),
            "e1c4a8f2: git ls-files failed with status {}",
            command_output.status
        );
        let mut tracked_file_paths = Vec::new();
        for tracked_file_path_bytes in command_output.stdout.split(|byte| *byte == 0) {
            if tracked_file_path_bytes.is_empty() {
                continue;
            }
            let tracked_file_path_text = String::from_utf8_lossy(tracked_file_path_bytes);
            tracked_file_paths.push(workspace_root_path.join(tracked_file_path_text.as_ref()));
        }
        let mut tracked_files_with_non_ascii_symbols = Vec::new();
        for tracked_file_path in tracked_file_paths {
            if !tracked_file_path.exists() {
                continue;
            }
            let file_bytes = fs::read(&tracked_file_path).expect("f4b2e8a1");
            if !file_bytes.is_ascii() {
                tracked_files_with_non_ascii_symbols.push(tracked_file_path);
            }
        }

        assert!(
            tracked_files_with_non_ascii_symbols.is_empty(),
            "2b4c6d8e: non-ASCII symbols found in tracked files: \
             {tracked_files_with_non_ascii_symbols:?}"
        );
    }
}
