# Telegram Agent Job Notifyer Client

Клиент для отправки уведомлений о выполнении заданий в Telegram.

## Использование

```rust
use client::notify;

// Простое уведомление
notify(&client, "http://localhost:8080/notify", Some("Backup completed"), None).await?;
```
