use std::{
    env,
    io::{Write as _, stderr},
    path::PathBuf,
    process::ExitCode,
};

use codex_task_runner_shared::{
    DEFAULT_TASKS_FILE_NAME, TaskRunnerConfiguration, resolve_codex_binary_from_environment,
    resolve_log_maximum_bytes_from_environment, run_tasks_json_file,
};

fn main() -> ExitCode {
    match (|| -> Result<(), String> {
        let mut command_line_arguments = env::args_os();
        let _binary_name = command_line_arguments.next();
        let tasks_file_path = command_line_arguments.next().map_or_else(
            || Ok(PathBuf::from(DEFAULT_TASKS_FILE_NAME)),
            |tasks_file_os_string| {
                if command_line_arguments.next().is_some() {
                    return Err(String::from(
                        "6b0d3f5a at most 1 argument is allowed: <tasks-json-file>",
                    ));
                }
                Ok(PathBuf::from(tasks_file_os_string))
            },
        )?;
        let codex_binary_path = resolve_codex_binary_from_environment();
        let log_maximum_bytes = resolve_log_maximum_bytes_from_environment()?;
        let task_runner_configuration =
            TaskRunnerConfiguration::cli_default(codex_binary_path, log_maximum_bytes);
        run_tasks_json_file(tasks_file_path.as_path(), &task_runner_configuration)
    })() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            drop(stderr().write_all(format!("{error}\n").as_bytes()));
            ExitCode::FAILURE
        }
    }
}
