use serde::{Deserialize, Serialize};

use crate::shared::{SenderUsername, TelegramMessageText};
#[derive(Debug, Clone, Deserialize)]
pub struct TelegramGetUpdatesResponse {
    pub description: Option<String>,
    pub ok: bool,
    pub result: Vec<TelegramUpdate>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct TelegramUpdate {
    pub message: Option<TelegramIncomingMessage>,
    #[serde(rename = "update_id")]
    pub update_identifier: i64,
}
#[derive(Debug, Clone, Deserialize)]
pub struct TelegramIncomingMessage {
    pub chat: TelegramChat,
    pub from: Option<TelegramUser>,
    pub text: Option<TelegramMessageText>,
}
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct TelegramChat {
    #[serde(rename = "id")]
    pub chat_identifier: i64,
}
#[derive(Debug, Clone, Deserialize)]
pub struct TelegramUser {
    pub username: Option<SenderUsername>,
}
#[derive(Debug, Clone, Serialize)]
pub struct TelegramSendMessageRequest {
    #[serde(rename = "chat_id")]
    pub chat_identifier: i64,
    pub text: String,
}
#[derive(Debug, Clone, Deserialize)]
pub struct TelegramSendMessageResponse {
    pub description: Option<String>,
    pub ok: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalUpdate {
    pub chat_identifier: i64,
    pub message_text: TelegramMessageText,
    pub sender_username: Option<SenderUsername>,
    pub update_identifier: i64,
}
#[must_use]
pub fn convert_telegram_update_to_internal(
    telegram_update: TelegramUpdate,
) -> Option<InternalUpdate> {
    let incoming_message = telegram_update.message?;
    let message_text = incoming_message.text?;
    Some(InternalUpdate {
        chat_identifier: incoming_message.chat.chat_identifier,
        message_text,
        sender_username: incoming_message.from.and_then(|sender| sender.username),
        update_identifier: telegram_update.update_identifier,
    })
}
#[cfg(test)]
mod tests {
    use super::{
        InternalUpdate, TelegramChat, TelegramIncomingMessage, TelegramUpdate, TelegramUser,
        convert_telegram_update_to_internal,
    };
    use crate::shared::{SenderUsername, TelegramMessageText};
    #[test]
    fn conversion_returns_expected_internal_update() {
        let telegram_update = TelegramUpdate {
            message: Some(TelegramIncomingMessage {
                chat: TelegramChat {
                    chat_identifier: 111,
                },
                from: Some(TelegramUser {
                    username: Some(SenderUsername::from(String::from("kuqmua"))),
                }),
                text: Some(TelegramMessageText::from(String::from("/health"))),
            }),
            update_identifier: 42,
        };
        assert_eq!(
            convert_telegram_update_to_internal(telegram_update),
            Some(InternalUpdate {
                chat_identifier: 111,
                message_text: TelegramMessageText::from(String::from("/health")),
                sender_username: Some(SenderUsername::from(String::from("kuqmua"))),
                update_identifier: 42,
            })
        );
    }
    #[test]
    fn conversion_returns_none_for_missing_message() {
        let telegram_update = TelegramUpdate {
            message: None,
            update_identifier: 7,
        };
        assert!(convert_telegram_update_to_internal(telegram_update).is_none());
    }
}
