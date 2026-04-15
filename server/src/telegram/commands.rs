use crate::shared::{
    IncomingCommand, IncomingCommandName, TelegramMessageText, parse_incoming_command,
};
#[must_use]
pub fn parse_command(message_text: &TelegramMessageText) -> IncomingCommand {
    parse_incoming_command(message_text)
}
#[must_use]
pub const fn command_name(command: &IncomingCommand) -> IncomingCommandName {
    match command {
        IncomingCommand::Active => IncomingCommandName::new("active"),
        IncomingCommand::Cancel(_) => IncomingCommandName::new("cancel"),
        IncomingCommand::Health => IncomingCommandName::new("health"),
        IncomingCommand::Codex(_) => IncomingCommandName::new("codex"),
        IncomingCommand::CodexDebug(_) => IncomingCommandName::new("debug"),
        IncomingCommand::CodexDebugPromptInput(_) => IncomingCommandName::new("debug_prompt_input"),
        IncomingCommand::CodexFeatures(_) => IncomingCommandName::new("features"),
        IncomingCommand::CodexFeaturesList => IncomingCommandName::new("features_list"),
        IncomingCommand::CodexMcpList => IncomingCommandName::new("mcp_list"),
        IncomingCommand::CodexSandbox(_) => IncomingCommandName::new("sandbox"),
        IncomingCommand::CodexProcess(_) => IncomingCommandName::new("codex_process"),
        IncomingCommand::Openai(_) => IncomingCommandName::new("openai"),
        IncomingCommand::Help => IncomingCommandName::new("help"),
        IncomingCommand::Invalid { command_name, .. } => IncomingCommandName::new(command_name),
        IncomingCommand::Last => IncomingCommandName::new("last"),
        IncomingCommand::Limits => IncomingCommandName::new("limits"),
        IncomingCommand::List => IncomingCommandName::new("list"),
        IncomingCommand::Output(_) => IncomingCommandName::new("output"),
        IncomingCommand::OpenaiUrls => IncomingCommandName::new("openai_urls"),
        IncomingCommand::Queue => IncomingCommandName::new("queue"),
        IncomingCommand::Retry(_) => IncomingCommandName::new("retry"),
        IncomingCommand::Stats => IncomingCommandName::new("stats"),
        IncomingCommand::Status(_) => IncomingCommandName::new("status"),
        IncomingCommand::Unknown => IncomingCommandName::new("unknown"),
        IncomingCommand::Version => IncomingCommandName::new("version"),
        IncomingCommand::WhoAmI => IncomingCommandName::new("whoami"),
    }
}
#[cfg(test)]
mod tests {
    use super::parse_command;
    use crate::shared::{IncomingCommand, PromptText, TaskIdentifier, TelegramMessageText};

    fn make_telegram_message_text(message_text: &str) -> TelegramMessageText {
        TelegramMessageText::from(String::from(message_text))
    }

    #[test]
    fn parse_health_command() {
        assert_eq!(parse_command(&make_telegram_message_text("/health")), IncomingCommand::Health);
    }
    #[test]
    fn parse_codex_command_with_prompt() {
        assert_eq!(
            parse_command(&make_telegram_message_text("/codex describe ownership")),
            IncomingCommand::Codex(PromptText::from(String::from("describe ownership")))
        );
    }

    #[test]
    fn parse_codex_process_command_with_prompt() {
        assert_eq!(
            parse_command(&make_telegram_message_text("/codex_process describe ownership")),
            IncomingCommand::CodexProcess(PromptText::from(String::from("describe ownership")))
        );
    }

    #[test]
    fn parse_openai_command_with_prompt() {
        assert_eq!(
            parse_command(&make_telegram_message_text("/openai describe ownership")),
            IncomingCommand::Openai(PromptText::from(String::from("describe ownership")))
        );
    }

    #[test]
    fn parse_openai_uniform_resource_locators_command() {
        assert_eq!(
            parse_command(&make_telegram_message_text("/openai_urls")),
            IncomingCommand::OpenaiUrls
        );
    }

    #[test]
    fn parse_sandbox_command_with_arguments() {
        assert_eq!(
            parse_command(&make_telegram_message_text("/sandbox linux echo hello")),
            IncomingCommand::CodexSandbox(PromptText::from(String::from("linux echo hello")))
        );
    }

    #[test]
    fn parse_debug_command_with_arguments() {
        assert_eq!(
            parse_command(&make_telegram_message_text("/debug app-server")),
            IncomingCommand::CodexDebug(PromptText::from(String::from("app-server")))
        );
    }

    #[test]
    fn parse_features_command_with_arguments() {
        assert_eq!(
            parse_command(&make_telegram_message_text("/features list")),
            IncomingCommand::CodexFeatures(PromptText::from(String::from("list")))
        );
    }

    #[test]
    fn parse_mcp_list_command() {
        assert_eq!(
            parse_command(&make_telegram_message_text("/mcp_list")),
            IncomingCommand::CodexMcpList
        );
    }

    #[test]
    fn parse_debug_prompt_input_command() {
        assert_eq!(
            parse_command(&make_telegram_message_text("/debug_prompt_input prompt text")),
            IncomingCommand::CodexDebugPromptInput(PromptText::from(String::from("prompt text")))
        );
    }

    #[test]
    fn parse_features_list_command() {
        assert_eq!(
            parse_command(&make_telegram_message_text("/features_list")),
            IncomingCommand::CodexFeaturesList
        );
    }

    #[test]
    fn parse_status_command() {
        assert_eq!(
            parse_command(&make_telegram_message_text("/status 1")),
            IncomingCommand::Status(TaskIdentifier::from(1))
        );
    }
    #[test]
    fn parse_whoami_command() {
        assert_eq!(parse_command(&make_telegram_message_text("/whoami")), IncomingCommand::WhoAmI);
    }
    #[test]
    fn parse_unknown_command() {
        assert_eq!(parse_command(&make_telegram_message_text("hello")), IncomingCommand::Unknown);
    }

    #[test]
    fn parse_version_command() {
        assert_eq!(
            parse_command(&make_telegram_message_text("/version")),
            IncomingCommand::Version
        );
    }

    #[test]
    fn parse_output_command() {
        assert_eq!(
            parse_command(&make_telegram_message_text("/output 7")),
            IncomingCommand::Output(TaskIdentifier::from(7))
        );
    }

    #[test]
    fn parse_queue_command() {
        assert_eq!(parse_command(&make_telegram_message_text("/queue")), IncomingCommand::Queue);
    }

    #[test]
    fn parse_stats_command() {
        assert_eq!(parse_command(&make_telegram_message_text("/stats")), IncomingCommand::Stats);
    }
}
