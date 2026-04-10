use axum as _;
use codex_cli as _;
use dotenvy as _;
use reqwest as _;
use serde as _;
use serde_json as _;
use server as _;
use shared as _;
use thiserror as _;
use tokio as _;
use tracing as _;
use tracing_subscriber as _;

#[cfg(test)]
mod tests {
    use std::{
        fs, io,
        path::{Path, PathBuf},
    };

    fn collect_rust_source_files(
        directory_path: &Path,
        rust_source_file_paths: &mut Vec<PathBuf>,
    ) -> io::Result<()> {
        for directory_entry_result in fs::read_dir(directory_path)? {
            let directory_entry = directory_entry_result?;
            let entry_path = directory_entry.path();
            if entry_path.is_dir() {
                let directory_name = directory_entry.file_name();
                let directory_name_text = directory_name.to_string_lossy();
                if directory_name_text == "target" || directory_name_text == ".git" {
                    continue;
                }
                collect_rust_source_files(&entry_path, rust_source_file_paths)?;
                continue;
            }
            let file_extension = entry_path
                .extension()
                .and_then(|extension| extension.to_str());
            if file_extension == Some("rs") {
                rust_source_file_paths.push(entry_path);
            }
        }
        Ok(())
    }

    #[test]
    fn all_rust_source_files_use_ascii_symbols_only() {
        let workspace_root_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map_or_else(PathBuf::new, PathBuf::from);
        assert!(
            !workspace_root_path.as_os_str().is_empty(),
            "f1a2b3c4: failed to resolve workspace root path"
        );

        let mut rust_source_file_paths = Vec::new();
        collect_rust_source_files(&workspace_root_path, &mut rust_source_file_paths)
            .expect("d3a9f1c7");

        let mut files_with_non_ascii_symbols = Vec::new();
        for rust_source_file_path in rust_source_file_paths {
            let source_bytes = fs::read(&rust_source_file_path).expect("f4b2e8a1");
            if !source_bytes.is_ascii() {
                files_with_non_ascii_symbols.push(rust_source_file_path);
            }
        }

        assert!(
            files_with_non_ascii_symbols.is_empty(),
            "2b4c6d8e: non-ASCII symbols found in Rust source files: \
             {files_with_non_ascii_symbols:?}"
        );
    }
}
