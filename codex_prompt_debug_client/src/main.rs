use reqwest::Client;

const SERVER_URL: &str = "http://localhost:8080/notify";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = Client::new();
    codex_prompt_debug_client::send_codex_prompt_for_debug(&client, SERVER_URL, Some("MEOW"), None)
        .await?;
    Ok(())
}
