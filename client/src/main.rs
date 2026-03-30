use reqwest::Client;

const SERVER_URL: &str = "http://localhost:8080/notify";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();

    // Пример 1: Простое уведомление об успехе
    client::notify(&client, SERVER_URL, "backup", "completed", Some("Backup completed"), None)
        .await?;

    // Пример 2: Уведомление об ошибке
    client::notify(&client, SERVER_URL, "sync", "failed", None, Some("Connection timeout")).await?;

    // Пример 3: Автоматическое обёртывание с замером времени
    client::run_with_notify(&client, SERVER_URL, "data-pipeline", || async {
        // Симуляция работы
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        Ok("Processed 1500 records".to_string())
    })
    .await?;

    // Пример 4: Задача с ошибкой
    let _ = client::run_with_notify(&client, SERVER_URL, "migration", || async {
        Err("Database connection lost".into())
    })
    .await;

    Ok(())
}
