use serde::{Deserialize, Serialize};

pub const SYSTEM_MESSAGE_PREFIX: &str = "[telegram-agent]";
pub const SYSTEM_MESSAGE_HEALTHY: &str = "Health check: bot is alive";
pub const SYSTEM_MESSAGE_CODEX_USAGE: &str = "Usage: /codex <prompt>";
pub const SYSTEM_MESSAGE_CODEX_STARTED: &str = "Work started";
pub const SYSTEM_MESSAGE_CODEX_FINISHED: &str = "Work finished";
pub const SYSTEM_MESSAGE_UNKNOWN_COMMAND: &str = "Unknown command";
pub const SYSTEM_MESSAGE_EMPTY_CODEX_OUTPUT: &str = "(empty codex output)";
pub const SYSTEM_MESSAGE_TRUNCATED_SUFFIX: &str = "\n...[truncated]";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncomingCommand {
    Codex(String),
    Health,
    Unknown,
}

#[must_use]
pub fn parse_incoming_command(input_text: &str) -> IncomingCommand {
    let trimmed_input_text = input_text.trim();
    if trimmed_input_text == "/health" {
        return IncomingCommand::Health;
    }
    if let Some(raw_prompt) = trimmed_input_text.strip_prefix("/codex") {
        return IncomingCommand::Codex(raw_prompt.trim().to_owned());
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

#[cfg(test)]
mod tests {
    use super::{
        IncomingCommand, normalize_codex_output, parse_incoming_command, split_text_into_chunks,
    };

    #[test]
    fn parse_command_health() {
        assert_eq!(parse_incoming_command(" /health "), IncomingCommand::Health);
    }

    #[test]
    fn parse_command_codex() {
        assert_eq!(
            parse_incoming_command("/codex  explain rust ownership"),
            IncomingCommand::Codex(String::from("explain rust ownership"))
        );
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
}
