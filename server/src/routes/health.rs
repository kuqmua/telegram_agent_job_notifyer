#[allow(
    clippy::single_call_fn,
    reason = "Handler is wired exactly once in router setup by design"
)]
pub(crate) async fn handle() -> &'static str {
    tracing::info!("route=/health msg=healthcheck");
    "OK"
}
