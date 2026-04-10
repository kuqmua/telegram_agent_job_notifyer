use shared::{IncomingCommand, parse_incoming_command};
#[must_use]
pub fn parse_command(message_text: &str) -> IncomingCommand {
    parse_incoming_command(message_text)
}
#[must_use]
pub const fn command_name(command: &IncomingCommand) -> &'static str {
    match command {
        IncomingCommand::Active => "active",
        IncomingCommand::Cancel(_) => "cancel",
        IncomingCommand::Health => "health",
        IncomingCommand::Codex(_) => "codex",
        IncomingCommand::Help => "help",
        IncomingCommand::Invalid { command_name, .. } => command_name,
        IncomingCommand::Limits => "limits",
        IncomingCommand::List => "list",
        IncomingCommand::Retry(_) => "retry",
        IncomingCommand::Status(_) => "status",
        IncomingCommand::Unknown => "unknown",
    }
}
#[cfg(test)]
mod tests {
    use shared::IncomingCommand;

    use super::parse_command;
    #[test]
    fn parse_health_command() {
        assert_eq!(parse_command("/health"), IncomingCommand::Health);
    }
    #[test]
    fn parse_codex_command_with_prompt() {
        assert_eq!(
            parse_command("/codex describe ownership"),
            IncomingCommand::Codex(String::from("describe ownership"))
        );
    }
    #[test]
    fn parse_status_command() {
        assert_eq!(parse_command("/status 1"), IncomingCommand::Status(1));
    }
    #[test]
    fn parse_unknown_command() {
        assert_eq!(parse_command("hello"), IncomingCommand::Unknown);
    }
}
