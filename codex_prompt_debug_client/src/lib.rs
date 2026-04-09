use reqwest::Client;
use shared::JobPayload;
pub async fn send_codex_prompt_for_debug(
    client: &Client,
    server_url: &str,
    result: Option<&str>,
    error: Option<&str>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(result_text) = result {
        codex_cli::exec_prompt(result_text)?;
    }
    let payload = JobPayload {
        error: error.map(|s| s.into()),
        result: result.map(|s| s.into()),
    };
    client
        .post(server_url)
        .json(&payload)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}
