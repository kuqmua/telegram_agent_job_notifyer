use crate::shared::{IncomingCommand, parse_incoming_command};
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
        IncomingCommand::Last => "last",
        IncomingCommand::Limits => "limits",
        IncomingCommand::List => "list",
        IncomingCommand::Output(_) => "output",
        IncomingCommand::Queue => "queue",
        IncomingCommand::Retry(_) => "retry",
        IncomingCommand::Stats => "stats",
        IncomingCommand::Status(_) => "status",
        IncomingCommand::Unknown => "unknown",
        IncomingCommand::Version => "version",
        IncomingCommand::WhoAmI => "whoami",
    }
}
#[cfg(test)]
mod tests {
    use super::parse_command;
    use crate::shared::IncomingCommand;
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
    fn parse_whoami_command() {
        assert_eq!(parse_command("/whoami"), IncomingCommand::WhoAmI);
    }
    #[test]
    fn parse_unknown_command() {
        assert_eq!(parse_command("hello"), IncomingCommand::Unknown);
    }

    #[test]
    fn parse_version_command() {
        assert_eq!(parse_command("/version"), IncomingCommand::Version);
    }

    #[test]
    fn parse_output_command() {
        assert_eq!(parse_command("/output 7"), IncomingCommand::Output(7));
    }

    #[test]
    fn parse_queue_command() {
        assert_eq!(parse_command("/queue"), IncomingCommand::Queue);
    }

    #[test]
    fn parse_stats_command() {
        assert_eq!(parse_command("/stats"), IncomingCommand::Stats);
    }
}
