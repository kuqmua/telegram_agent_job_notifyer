pub mod status {
    use axum::{
        extract::State,
        http::StatusCode as HyperTextTransferProtocolStatusCode,
        response::{IntoResponse as _, Response},
    };

    use crate::runtime::ServiceState;
    pub async fn live_probe() -> &'static str {
        "OK"
    }
    pub async fn ready_probe(State(runtime_state): State<ServiceState>) -> Response {
        if runtime_state.is_polling_ready() {
            return (HyperTextTransferProtocolStatusCode::OK, "READY").into_response();
        }
        (HyperTextTransferProtocolStatusCode::SERVICE_UNAVAILABLE, "NOT_READY").into_response()
    }
    pub async fn health_probe(State(runtime_state): State<ServiceState>) -> Response {
        ready_probe(State(runtime_state)).await
    }
    pub async fn metrics_probe(State(runtime_state): State<ServiceState>) -> Response {
        let metrics_response = runtime_state.metrics().render_prometheus(
            runtime_state.is_polling_ready(),
            runtime_state.configured_telegram_chat_identifier(),
        );
        (HyperTextTransferProtocolStatusCode::OK, metrics_response).into_response()
    }
}
