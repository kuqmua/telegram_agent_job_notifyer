use std::error::Error;

use reqwest::Client;
use shared::JobPayload;

const SERVER_URL: &str = "http://localhost:8080/notify";

#[tokio::main]
#[allow(clippy::unwrap_in_result)]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let client = Client::new();
    let result = Some("создай простой html файл");
    let error = None;
    if let Some(result_text) = result {
        codex_cli::exec_prompt(result_text)?;
    }
    let payload = JobPayload {
        error: error.map(|value: &str| value.into()),
        result: result.map(|value: &str| value.into()),
    };
    let _response = client
        .post(SERVER_URL)
        .json(&payload)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}
