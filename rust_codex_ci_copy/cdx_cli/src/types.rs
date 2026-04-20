use std::time::Instant;

#[derive(Clone, Debug)]
pub(crate) struct AppCfg {
    pub server: String,
    pub tasks: Vec<TaskSpec>,
}

#[derive(Clone, Debug)]
pub(crate) struct ProgressEntry {
    pub current_iteration: u32,
    pub finished: bool,
    pub iteration_started_at: Option<Instant>,
    pub prompt: String,
    pub task_started_at_unix: u64,
    pub total_repeat: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct TaskSpec {
    pub prompt: String,
    pub repeat: u32,
}
