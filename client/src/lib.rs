//! Клиент для отправки уведомлений о выполнении заданий.
//!
//! # Пример
//!
//! ```no_run
//! use reqwest::Client;
//! use tokio::runtime::Runtime;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let rt = Runtime::new()?;
//! let client = Client::new();
//!
//! rt.block_on(async {
//!     client::notify(
//!         &client,
//!         "http://localhost:8080/notify",
//!         "backup",
//!         "completed",
//!         Some("Backup done"),
//!         None,
//!     )
//!     .await?;
//!     Ok::<(), Box<dyn std::error::Error>>(())
//! })?;
//! # Ok(())
//! # }
//! ```

use reqwest::Client;
use serde::Serialize;

#[derive(Serialize)]
struct NotifyPayload {
    agent_name: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    elapsed_ms: Option<u64>,
}

/// Отправляет уведомление на сервер.
pub async fn notify(
    client: &Client,
    server_url: &str,
    agent_name: &str,
    status: &str,
    result: Option<&str>,
    error: Option<&str>,
) -> Result<(), reqwest::Error> {
    let payload = NotifyPayload {
        agent_name: agent_name.into(),
        status: status.into(),
        result: result.map(|s| s.into()),
        error: error.map(|s| s.into()),
        elapsed_ms: None,
    };
    client.post(server_url).json(&payload).send().await?;
    Ok(())
}

/// Выполняет задачу с автоматическим замером времени и отправкой уведомления.
pub async fn run_with_notify<F, Fut>(
    client: &Client,
    server_url: &str,
    agent_name: &str,
    f: F,
) -> Result<String, Box<dyn std::error::Error>>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<String, Box<dyn std::error::Error>>>,
{
    use std::time::Instant;
    let start = Instant::now();
    let result = f().await;
    let elapsed = start.elapsed().as_millis() as u64;

    let payload = match &result {
        Ok(msg) => NotifyPayload {
            agent_name: agent_name.into(),
            status: "completed".into(),
            result: Some(msg.clone()),
            error: None,
            elapsed_ms: Some(elapsed),
        },
        Err(e) => NotifyPayload {
            agent_name: agent_name.into(),
            status: "failed".into(),
            result: None,
            error: Some(e.to_string()),
            elapsed_ms: Some(elapsed),
        },
    };

    client.post(server_url).json(&payload).send().await?;
    result
}
