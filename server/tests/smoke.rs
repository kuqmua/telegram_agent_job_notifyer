use codex_cli as _;
use dotenvy as _;
use serde as _;
use serde_json as _;
use shared as _;
use thiserror as _;
use tracing as _;
use tracing_subscriber as _;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use server::{build_router, build_runtime_state, settings::ServiceConfiguration};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn health_endpoint_is_available() {
        let environment_variables = BTreeMap::from([
            (
                String::from("TELEGRAM_BOT_TOKEN"),
                String::from("123456789:ABCDEFGHIJKLMNOPQRSTUVWXYZ"),
            ),
            (String::from("HOST"), String::from("127.0.0.1")),
            (String::from("PORT"), String::from("8080")),
        ]);
        let runtime_settings =
            ServiceConfiguration::from_environment_map(&environment_variables).expect("51e4a8fd");
        let runtime_state = build_runtime_state(&runtime_settings).expect("3f9b7c2a");
        runtime_state.set_polling_ready(true);

        let application_router = build_router(runtime_state);

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("90bc5a11");
        let listener_address = listener.local_addr().expect("f0de31aa");

        let server_task = tokio::spawn(async move {
            drop(axum::serve(listener, application_router).await);
        });

        let response = reqwest::get(format!("http://{listener_address}/health"))
            .await
            .expect("b35da761");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        server_task.abort();
    }
}
