# Telegram Agent Job Notifyer Client

Клиент для отправки уведомлений о выполнении заданий в Telegram.

## Использование

```rust
use client::{notify, run_with_notify};

// Простое уведомление
notify(&client, "backup", "completed", Some("Backup completed"), None).await?;

// С автоматическим замером времени
run_with_notify(&client, "data-pipeline", || async {
    // ваша задача
    Ok("Processed 1500 records".to_string())
}).await?;
```
