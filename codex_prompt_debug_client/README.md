# Telegram Agent Job Notifyer Client

Клиент для отправки уведомлений о выполнении заданий в Telegram.

## Использование

```rust
use codex_prompt_debug_client::send_codex_prompt_for_debug;

// Простое уведомление
send_codex_prompt_for_debug(&client, "http://localhost:8080/notify", Some("Backup completed"), None).await?;
```
