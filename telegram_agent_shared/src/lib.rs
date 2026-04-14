use std::{fmt, ops::Deref};

pub use codex_command_runtime::{
    CodexExecutionIsolation, PromptExecutionOutcome, exec_prompt, exec_prompt_capture,
    exec_prompt_capture_limited, exec_prompt_capture_limited_with_binary,
    exec_prompt_capture_limited_with_binary_and_control,
    exec_prompt_capture_limited_with_binary_and_control_with_json_output,
    exec_prompt_capture_limited_with_binary_and_control_with_json_output_and_progress,
};
use serde::{Deserialize, Serialize};

pub const SYSTEM_MESSAGE_PREFIX: &str = "[telegram-agent]";
pub const SYSTEM_MESSAGE_HEALTHY: &str = "Health check: bot is alive";
pub const SYSTEM_MESSAGE_HELP: &str =
    "Commands:\n/health - bot health\n/help - this help\n/codex <prompt> - create \
     task\n/codex_process <prompt> - create task with codex process output\n/openai <prompt> - \
     run prompt via OpenAI API\n/openai --configuration <index> <prompt> - select OpenAI \
     configuration from config\n/openai <system_prompt> || <prompt> - run prompt via OpenAI API \
     with system prompt\n/openai_urls - show configured OpenAI API URLs\n/status <task_id> - task \
     details\n/list - recent tasks\n/active - active tasks\n/cancel <task_id> - cancel \
     task\n/retry <task_id> - retry task\n/output <task_id> - task output only\n/last - latest \
     task\n/queue - queue status\n/stats - task counters\n/limits - runtime limits\n/whoami - \
     sender identity\n/version - build info\n\nExamples:\n/codex explain ownership in \
     rust\n/codex_process explain ownership in rust\n/openai explain ownership in rust\n/openai \
     you are strict reviewer || explain ownership in rust\n/openai --configuration 2 explain \
     ownership in rust\n/openai_urls\n/status 42\n/output 42\n/retry 42";
pub const SYSTEM_MESSAGE_CODEX_USAGE: &str = "Usage: /codex <prompt>";
pub const SYSTEM_MESSAGE_CODEX_PROCESS_USAGE: &str = "Usage: /codex_process <prompt>";
pub const SYSTEM_MESSAGE_OPENAI_USAGE: &str = "Usage: /openai [--configuration <index>] <prompt> \
                                               or /openai [--configuration <index>] \
                                               <system_prompt> || <prompt>";
pub const SYSTEM_MESSAGE_OPENAI_URLS_EMPTY: &str = "No configured OpenAI API URLs";
pub const SYSTEM_MESSAGE_OPENAI_NOT_CONFIGURED: &str =
    "OpenAI command is not configured: set OPENAI_CONFIGURATIONS";
pub const SYSTEM_MESSAGE_OPENAI_TIMED_OUT: &str = "OpenAI request timed out";
pub const SYSTEM_MESSAGE_CODEX_STARTED: &str = "Task started";
pub const SYSTEM_MESSAGE_CODEX_QUEUED: &str = "Task queued";
pub const SYSTEM_MESSAGE_CODEX_FINISHED: &str = "Task finished";
pub const SYSTEM_MESSAGE_CODEX_BUSY: &str = "Task is still running, please wait";
pub const SYSTEM_MESSAGE_CODEX_CANCELLED: &str = "Task cancelled";
pub const SYSTEM_MESSAGE_CODEX_TIMED_OUT: &str = "Task timed out";
pub const SYSTEM_MESSAGE_UNKNOWN_COMMAND: &str = "Unknown command";
pub const SYSTEM_MESSAGE_INVALID_COMMAND_ARGUMENTS: &str = "Invalid command arguments";
pub const SYSTEM_MESSAGE_TASK_NOT_FOUND: &str = "Task not found";
pub const SYSTEM_MESSAGE_TASK_ACCESS_DENIED: &str = "Task access denied";
pub const SYSTEM_MESSAGE_TASK_RATE_LIMITED: &str = "Task rate limit exceeded";
pub const SYSTEM_MESSAGE_TASK_PROMPT_TOO_LONG: &str = "Prompt too long";
pub const SYSTEM_MESSAGE_TASK_QUEUE_WAIT_EXCEEDED: &str =
    "Task cancelled: queue wait limit exceeded";
pub const SYSTEM_MESSAGE_USERNAME_REQUIRED: &str = "username required";
pub const SYSTEM_MESSAGE_NO_ACTIVE_TASKS: &str = "No active tasks";
pub const SYSTEM_MESSAGE_NO_TASKS: &str = "No tasks";
pub const SYSTEM_MESSAGE_EMPTY_CODEX_OUTPUT: &str = "(empty codex output)";
pub const SYSTEM_MESSAGE_TRUNCATED_SUFFIX: &str = "\n...[truncated]";
pub const ERROR_MESSAGE_CODEX_EXECUTION_PREFIX: &str = "codex error";
pub const ERROR_MESSAGE_CODEX_PERMIT_PREFIX: &str = "codex permit error";
pub const ERROR_MESSAGE_CODEX_TASK_JOIN_PREFIX: &str = "codex task error";
pub const ERROR_MESSAGE_SEMAPHORE_CLOSED: &str = "semaphore closed";
pub const ERROR_MESSAGE_TASK_PROMPT_NOT_FOUND: &str = "task prompt not found";
pub const VALUE_NONE: &str = "none";
pub const SYSTEM_MESSAGES_ALL: [&str; 27] = [
    SYSTEM_MESSAGE_PREFIX,
    SYSTEM_MESSAGE_HEALTHY,
    SYSTEM_MESSAGE_HELP,
    SYSTEM_MESSAGE_CODEX_USAGE,
    SYSTEM_MESSAGE_CODEX_PROCESS_USAGE,
    SYSTEM_MESSAGE_OPENAI_USAGE,
    SYSTEM_MESSAGE_OPENAI_URLS_EMPTY,
    SYSTEM_MESSAGE_OPENAI_NOT_CONFIGURED,
    SYSTEM_MESSAGE_OPENAI_TIMED_OUT,
    SYSTEM_MESSAGE_CODEX_STARTED,
    SYSTEM_MESSAGE_CODEX_QUEUED,
    SYSTEM_MESSAGE_CODEX_FINISHED,
    SYSTEM_MESSAGE_CODEX_BUSY,
    SYSTEM_MESSAGE_CODEX_CANCELLED,
    SYSTEM_MESSAGE_CODEX_TIMED_OUT,
    SYSTEM_MESSAGE_UNKNOWN_COMMAND,
    SYSTEM_MESSAGE_INVALID_COMMAND_ARGUMENTS,
    SYSTEM_MESSAGE_TASK_NOT_FOUND,
    SYSTEM_MESSAGE_TASK_ACCESS_DENIED,
    SYSTEM_MESSAGE_TASK_RATE_LIMITED,
    SYSTEM_MESSAGE_TASK_PROMPT_TOO_LONG,
    SYSTEM_MESSAGE_TASK_QUEUE_WAIT_EXCEEDED,
    SYSTEM_MESSAGE_USERNAME_REQUIRED,
    SYSTEM_MESSAGE_NO_ACTIVE_TASKS,
    SYSTEM_MESSAGE_NO_TASKS,
    SYSTEM_MESSAGE_EMPTY_CODEX_OUTPUT,
    SYSTEM_MESSAGE_TRUNCATED_SUFFIX,
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<TaskExecutionOutputText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<TaskExecutionOutputText>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PromptText(String);

impl PromptText {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn character_count(&self) -> usize {
        self.0.chars().count()
    }

    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<String> for PromptText {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl Deref for PromptText {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for PromptText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskExecutionOutputText(String);

impl TaskExecutionOutputText {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl From<String> for TaskExecutionOutputText {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl Deref for TaskExecutionOutputText {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for TaskExecutionOutputText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SenderUsername(String);

impl SenderUsername {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for SenderUsername {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl Deref for SenderUsername {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for SenderUsername {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TelegramMessageText(String);

impl TelegramMessageText {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for TelegramMessageText {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl Deref for TelegramMessageText {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for TelegramMessageText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidCommandMessage(String);

impl InvalidCommandMessage {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for InvalidCommandMessage {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl Deref for InvalidCommandMessage {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for InvalidCommandMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncomingCommand {
    Active,
    Cancel(u64),
    Codex(PromptText),
    CodexProcess(PromptText),
    Health,
    Help,
    Invalid {
        command_name: &'static str,
        message: InvalidCommandMessage,
    },
    Last,
    Limits,
    List,
    Openai(PromptText),
    OpenaiUrls,
    Output(u64),
    Queue,
    Retry(u64),
    Stats,
    Status(u64),
    Unknown,
    Version,
    WhoAmI,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodexTaskStatus {
    Cancelled,
    Failed,
    Queued,
    Running,
    Succeeded,
    TimedOut,
}

impl CodexTaskStatus {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Cancelled | Self::Failed | Self::Succeeded | Self::TimedOut)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskOwner {
    pub chat_identifier: i64,
    pub sender_username: Option<SenderUsername>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCreationRequest {
    pub owner: TaskOwner,
    pub prompt_text: PromptText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSummary {
    pub created_unix_milliseconds: u64,
    pub finished_unix_milliseconds: Option<u64>,
    pub owner: TaskOwner,
    pub started_unix_milliseconds: Option<u64>,
    pub status: CodexTaskStatus,
    pub task_identifier: u64,
}

#[must_use]
pub fn parse_incoming_command(input_text: &str) -> IncomingCommand {
    let trimmed_input_text = input_text.trim();
    if trimmed_input_text.eq_ignore_ascii_case("/health") {
        return IncomingCommand::Health;
    }
    if trimmed_input_text.eq_ignore_ascii_case("/help") {
        return IncomingCommand::Help;
    }
    if trimmed_input_text.eq_ignore_ascii_case("/list") {
        return IncomingCommand::List;
    }
    if trimmed_input_text.eq_ignore_ascii_case("/last") {
        return IncomingCommand::Last;
    }
    if trimmed_input_text.eq_ignore_ascii_case("/active") {
        return IncomingCommand::Active;
    }
    if trimmed_input_text.eq_ignore_ascii_case("/queue") {
        return IncomingCommand::Queue;
    }
    if trimmed_input_text.eq_ignore_ascii_case("/stats") {
        return IncomingCommand::Stats;
    }
    if trimmed_input_text.eq_ignore_ascii_case("/limits") {
        return IncomingCommand::Limits;
    }
    if trimmed_input_text.eq_ignore_ascii_case("/whoami") {
        return IncomingCommand::WhoAmI;
    }
    if trimmed_input_text.eq_ignore_ascii_case("/version") {
        return IncomingCommand::Version;
    }
    if trimmed_input_text.eq_ignore_ascii_case("/openai_urls") {
        return IncomingCommand::OpenaiUrls;
    }
    if let Some(raw_prompt) = trimmed_input_text.strip_prefix("/codex_process") {
        return IncomingCommand::CodexProcess(raw_prompt.trim().to_owned().into());
    }
    if let Some(raw_prompt) = trimmed_input_text.strip_prefix("/openai") {
        return IncomingCommand::Openai(raw_prompt.trim().to_owned().into());
    }
    if let Some(raw_prompt) = trimmed_input_text.strip_prefix("/codex") {
        return IncomingCommand::Codex(raw_prompt.trim().to_owned().into());
    }
    if let Some(command_arguments) = trimmed_input_text.strip_prefix("/status") {
        return parse_u64_command_argument("status", command_arguments)
            .map_or_else(|invalid_message| invalid_message, IncomingCommand::Status);
    }
    if let Some(command_arguments) = trimmed_input_text.strip_prefix("/cancel") {
        return parse_u64_command_argument("cancel", command_arguments)
            .map_or_else(|invalid_message| invalid_message, IncomingCommand::Cancel);
    }
    if let Some(command_arguments) = trimmed_input_text.strip_prefix("/retry") {
        return parse_u64_command_argument("retry", command_arguments)
            .map_or_else(|invalid_message| invalid_message, IncomingCommand::Retry);
    }
    if let Some(command_arguments) = trimmed_input_text.strip_prefix("/output") {
        return parse_u64_command_argument("output", command_arguments)
            .map_or_else(|invalid_message| invalid_message, IncomingCommand::Output);
    }
    IncomingCommand::Unknown
}

#[must_use]
pub fn format_system_message(message_text: &str) -> String {
    format!("{SYSTEM_MESSAGE_PREFIX} {message_text}")
}

#[must_use]
pub fn normalize_codex_output(raw_output: &str, maximum_characters: usize) -> String {
    let trimmed_output = raw_output.trim();
    if trimmed_output.is_empty() {
        return String::from(SYSTEM_MESSAGE_EMPTY_CODEX_OUTPUT);
    }
    let output_character_count = trimmed_output.chars().count();
    if output_character_count <= maximum_characters {
        return trimmed_output.to_owned();
    }
    let truncated_output = trimmed_output
        .chars()
        .take(maximum_characters)
        .collect::<String>();
    format!("{truncated_output}{SYSTEM_MESSAGE_TRUNCATED_SUFFIX}")
}

#[must_use]
pub fn split_text_into_chunks(
    message_text: &str,
    maximum_characters_per_chunk: usize,
) -> Vec<String> {
    if message_text.is_empty() {
        return vec![String::new()];
    }
    let mut result_chunks = Vec::new();
    let mut current_chunk = String::new();
    let mut current_chunk_character_count = 0usize;
    for current_character in message_text.chars() {
        if current_chunk_character_count >= maximum_characters_per_chunk {
            result_chunks.push(current_chunk);
            current_chunk = String::new();
            current_chunk_character_count = 0;
        }
        current_chunk.push(current_character);
        current_chunk_character_count = current_chunk_character_count.saturating_add(1);
    }
    if !current_chunk.is_empty() {
        result_chunks.push(current_chunk);
    }
    result_chunks
}

fn parse_u64_command_argument(
    command_name: &'static str,
    command_arguments: &str,
) -> Result<u64, IncomingCommand> {
    let trimmed_arguments = command_arguments.trim();
    if trimmed_arguments.is_empty() {
        return Err(IncomingCommand::Invalid {
            command_name,
            message: InvalidCommandMessage::from(String::from("task identifier is required")),
        });
    }
    match trimmed_arguments.parse::<u64>() {
        Ok(task_identifier) => Ok(task_identifier),
        Err(parse_error) => Err(IncomingCommand::Invalid {
            command_name,
            message: InvalidCommandMessage::from(format!(
                "task identifier must be u64: {parse_error}"
            )),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CodexTaskStatus, IncomingCommand, PromptText, SYSTEM_MESSAGES_ALL, normalize_codex_output,
        parse_incoming_command, split_text_into_chunks,
    };

    #[test]
    fn parse_command_health() {
        assert_eq!(parse_incoming_command(" /health "), IncomingCommand::Health);
    }

    #[test]
    fn parse_command_codex() {
        assert_eq!(
            parse_incoming_command("/codex  explain rust ownership"),
            IncomingCommand::Codex(PromptText::from(String::from("explain rust ownership")))
        );
    }

    #[test]
    fn parse_command_codex_process() {
        assert_eq!(
            parse_incoming_command("/codex_process  explain rust ownership"),
            IncomingCommand::CodexProcess(PromptText::from(
                String::from("explain rust ownership",)
            ))
        );
    }

    #[test]
    fn parse_command_openai() {
        assert_eq!(
            parse_incoming_command("/openai  explain rust ownership"),
            IncomingCommand::Openai(PromptText::from(String::from("explain rust ownership")))
        );
    }

    #[test]
    fn parse_command_openai_uniform_resource_locators() {
        assert_eq!(parse_incoming_command("/openai_urls"), IncomingCommand::OpenaiUrls);
    }

    #[test]
    fn parse_command_status() {
        assert_eq!(parse_incoming_command("/status 42"), IncomingCommand::Status(42));
    }

    #[test]
    fn parse_command_whoami() {
        assert_eq!(parse_incoming_command("/whoami"), IncomingCommand::WhoAmI);
    }

    #[test]
    fn parse_command_version() {
        assert_eq!(parse_incoming_command("/version"), IncomingCommand::Version);
    }

    #[test]
    fn parse_command_output() {
        assert_eq!(parse_incoming_command("/output 42"), IncomingCommand::Output(42));
    }

    #[test]
    fn parse_command_queue() {
        assert_eq!(parse_incoming_command("/queue"), IncomingCommand::Queue);
    }

    #[test]
    fn parse_command_stats() {
        assert_eq!(parse_incoming_command("/stats"), IncomingCommand::Stats);
    }

    #[test]
    fn parse_command_invalid_cancel() {
        let parsed_command = parse_incoming_command("/cancel abc");
        assert!(matches!(parsed_command, IncomingCommand::Invalid {
            command_name: "cancel",
            ..
        }));
    }

    #[test]
    fn parse_command_unknown() {
        assert_eq!(parse_incoming_command("/unknown"), IncomingCommand::Unknown);
    }

    #[test]
    fn normalize_output_empty() {
        assert_eq!(normalize_codex_output("   ", 10), "(empty codex output)");
    }

    #[test]
    fn normalize_output_truncated() {
        let normalized_output = normalize_codex_output("abcdef", 3);
        assert_eq!(normalized_output, "abc\n...[truncated]");
    }

    #[test]
    fn split_text_chunks() {
        let result_chunks = split_text_into_chunks("abcdef", 2);
        assert_eq!(result_chunks, vec!["ab", "cd", "ef"]);
    }

    #[test]
    fn codex_task_status_terminal() {
        assert!(CodexTaskStatus::Succeeded.is_terminal());
        assert!(!CodexTaskStatus::Running.is_terminal());
    }

    #[test]
    fn system_messages_use_ascii_symbols_only() {
        for system_message in SYSTEM_MESSAGES_ALL {
            assert!(
                system_message.is_ascii(),
                "system message contains non-ASCII symbols: {system_message}"
            );
        }
    }
}
