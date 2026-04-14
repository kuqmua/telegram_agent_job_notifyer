use dotenvy as _;
use serde as _;
use serde_json as _;
use telegram_agent_shared as _;
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
    #[tokio::test]
    async fn live_and_ready_endpoints_report_expected_status() {
        let environment_variables = BTreeMap::from([
            (
                String::from("TELEGRAM_BOT_TOKEN"),
                String::from("123456789:ABCDEFGHIJKLMNOPQRSTUVWXYZ"),
            ),
            (String::from("HOST"), String::from("127.0.0.1")),
            (String::from("PORT"), String::from("8080")),
        ]);
        let runtime_settings =
            ServiceConfiguration::from_environment_map(&environment_variables).expect("8a3e71b4");
        let runtime_state = build_runtime_state(&runtime_settings).expect("2f9cd8a1");
        runtime_state.set_polling_ready(false);
        let application_router = build_router(runtime_state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("f1c2a7d8");
        let listener_address = listener.local_addr().expect("9b4e3f2a");
        let server_task = tokio::spawn(async move {
            drop(axum::serve(listener, application_router).await);
        });
        let live_response = reqwest::get(format!("http://{listener_address}/health/live"))
            .await
            .expect("1d8f3a7c");
        assert_eq!(live_response.status(), reqwest::StatusCode::OK);
        let ready_response_not_ready =
            reqwest::get(format!("http://{listener_address}/health/ready"))
                .await
                .expect("7c1b4a9e");
        assert_eq!(ready_response_not_ready.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        runtime_state.set_polling_ready(true);
        let ready_response_ready = reqwest::get(format!("http://{listener_address}/health/ready"))
            .await
            .expect("4a7f2e1d");
        assert_eq!(ready_response_ready.status(), reqwest::StatusCode::OK);
        server_task.abort();
    }
}
