use std::time::Instant;

#[derive(Clone, Debug)]
pub(crate) struct AppCfg {
    pub tasks: Vec<TaskSpec>,
}

#[derive(Clone, Debug)]
pub(crate) struct ProgressEntry {
    pub current_iteration: u32,
    pub finished: bool,
    pub iteration_started_at: Option<Instant>,
}

#[derive(Clone, Debug)]
pub(crate) struct TaskSpec {
    pub prompt: String,
    pub repeat: u32,
}
