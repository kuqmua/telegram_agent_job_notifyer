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
use shared::JobPayload;

/// Отправляет уведомление на сервер.
pub async fn notify(
    client: &Client,
    server_url: &str,
    agent_name: &str,
    status: &str,
    result: Option<&str>,
    error: Option<&str>,
) -> Result<(), reqwest::Error> {
    let payload = JobPayload {
        agent_name: agent_name.into(),
        elapsed_ms: None,
        error: error.map(|s| s.into()),
        result: result.map(|s| s.into()),
        status: status.into(),
    };
    client
        .post(server_url)
        .json(&payload)
        .send()
        .await?
        .error_for_status()?;
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
        Ok(msg) => JobPayload {
            agent_name: agent_name.into(),
            elapsed_ms: Some(elapsed),
            error: None,
            result: Some(msg.clone()),
            status: "completed".into(),
        },
        Err(e) => JobPayload {
            agent_name: agent_name.into(),
            elapsed_ms: Some(elapsed),
            error: Some(e.to_string()),
            result: None,
            status: "failed".into(),
        },
    };

    client
        .post(server_url)
        .json(&payload)
        .send()
        .await?
        .error_for_status()?;
    result
}
