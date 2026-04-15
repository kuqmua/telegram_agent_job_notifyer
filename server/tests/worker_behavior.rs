use codex_command_runtime as _;
use dotenvy as _;
use openai_command_runtime as _;
use reqwest as _;
use serde as _;
use serde_json as _;
use telegram_agent_shared as _;
use thiserror as _;
use tracing as _;
use tracing_subscriber as _;

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, VecDeque},
        env, fs,
        net::SocketAddr,
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use axum::{
        Json, Router,
        extract::{Path, State},
        http::StatusCode as HyperTextTransferProtocolStatusCode,
        response::{IntoResponse as _, Response},
        routing::{get, post},
    };
    use serde_json::{Value, json};
    use server::{
        build_runtime_state,
        settings::ServiceConfiguration,
        shared::{
            SYSTEM_MESSAGE_CODEX_CANCELLED, SYSTEM_MESSAGE_CODEX_TIMED_OUT,
            SYSTEM_MESSAGE_TASK_QUEUE_WAIT_EXCEEDED, SYSTEM_MESSAGE_TASK_RATE_LIMITED,
            SYSTEM_MESSAGE_USERNAME_REQUIRED,
        },
        telegram::worker::run_updates_loop,
    };
    use tokio::{
        net::TcpListener,
        sync::{Mutex, watch},
        task::JoinHandle,
        time::sleep,
    };

    #[derive(Clone)]
    struct MockHyperTextTransferProtocolResponse {
        response_body: Value,
        status_code: HyperTextTransferProtocolStatusCode,
    }

    #[derive(Clone, Default)]
    struct MockTelegramState {
        get_updates_call_count: Arc<AtomicUsize>,
        get_updates_responses: Arc<Mutex<VecDeque<MockHyperTextTransferProtocolResponse>>>,
        openai_chat_completions_responses:
            Arc<Mutex<VecDeque<MockHyperTextTransferProtocolResponse>>>,
        openai_request_count: Arc<AtomicUsize>,
        send_message_responses: Arc<Mutex<VecDeque<MockHyperTextTransferProtocolResponse>>>,
        sent_message_count: Arc<AtomicUsize>,
        sent_messages: Arc<Mutex<Vec<String>>>,
    }

    fn build_environment(
        telegram_application_programming_interface_base_uniform_resource_locator: String,
        additional_environment: impl IntoIterator<Item = (&'static str, String)>,
    ) -> BTreeMap<String, String> {
        let mut environment_variables = BTreeMap::from([
            (
                String::from("TELEGRAM_BOT_TOKEN"),
                String::from("123456789:ABCDEFGHIJKLMNOPQRSTUVWXYZ"),
            ),
            (String::from("HOST"), String::from("127.0.0.1")),
            (String::from("PORT"), String::from("8080")),
            (
                String::from("TELEGRAM_API_BASE_URL"),
                telegram_application_programming_interface_base_uniform_resource_locator,
            ),
            (String::from("TELEGRAM_POLL_TIMEOUT_SECONDS"), String::from("1")),
            (String::from("TELEGRAM_POLL_BACKOFF_MIN_MS"), String::from("1")),
            (String::from("TELEGRAM_POLL_BACKOFF_MAX_MS"), String::from("5")),
            (String::from("CODEX_MAX_PARALLEL_TASKS"), String::from("1")),
            (String::from("UPDATE_MAX_PARALLEL_TASKS"), String::from("2")),
            (String::from("TASK_RATE_LIMIT_PER_MINUTE"), String::from("10")),
            (String::from("TASK_LIST_MAX_ITEMS"), String::from("10")),
        ]);
        for (environment_name, environment_value) in additional_environment {
            let _previous_value =
                environment_variables.insert(String::from(environment_name), environment_value);
        }
        environment_variables
    }

    async fn spawn_mock_telegram_server(
        mock_telegram_state: MockTelegramState,
    ) -> (SocketAddr, JoinHandle<()>) {
        let mock_application = Router::new()
            .route(
                "/bot{token}/getUpdates",
                get(
                    async |Path(_token): Path<String>,
                           State(route_state): State<MockTelegramState>|
                           -> Response {
                        let _previous_call_count = route_state
                            .get_updates_call_count
                            .fetch_add(1, Ordering::SeqCst);
                        let response = {
                            let mut response_guard = route_state.get_updates_responses.lock().await;
                            response_guard.pop_front().unwrap_or_else(|| {
                                MockHyperTextTransferProtocolResponse {
                                    response_body: json!({
                                        "ok": true,
                                        "result": []
                                    }),
                                    status_code: HyperTextTransferProtocolStatusCode::OK,
                                }
                            })
                        };
                        if response.status_code.is_success() {
                            return (response.status_code, Json(response.response_body))
                                .into_response();
                        }
                        (response.status_code, response.response_body.to_string()).into_response()
                    },
                ),
            )
            .route(
                "/bot{token}/sendMessage",
                post(
                    async |Path(_token): Path<String>,
                           State(route_state): State<MockTelegramState>,
                           Json(payload): Json<Value>|
                           -> Response {
                        if let Some(message_text) = payload.get("text").and_then(Value::as_str) {
                            let _previous_count = route_state
                                .sent_message_count
                                .fetch_add(1, Ordering::SeqCst);
                            route_state
                                .sent_messages
                                .lock()
                                .await
                                .push(String::from(message_text));
                        }
                        let response = {
                            let mut response_guard =
                                route_state.send_message_responses.lock().await;
                            response_guard.pop_front().unwrap_or_else(|| {
                                MockHyperTextTransferProtocolResponse {
                                    response_body: json!({
                                        "ok": true,
                                        "result": {}
                                    }),
                                    status_code: HyperTextTransferProtocolStatusCode::OK,
                                }
                            })
                        };
                        if response.status_code.is_success() {
                            return (response.status_code, Json(response.response_body))
                                .into_response();
                        }
                        (response.status_code, response.response_body.to_string()).into_response()
                    },
                ),
            )
            .route(
                "/openai/chat/completions",
                post(async |State(route_state): State<MockTelegramState>| -> Response {
                    let _previous_call_count = route_state
                        .openai_request_count
                        .fetch_add(1, Ordering::SeqCst);
                    let response = {
                        let mut response_guard =
                            route_state.openai_chat_completions_responses.lock().await;
                        response_guard.pop_front().unwrap_or_else(|| {
                            MockHyperTextTransferProtocolResponse {
                                response_body: json!({
                                    "choices": [
                                        {
                                            "message": {
                                                "content": "openai default response"
                                            }
                                        }
                                    ]
                                }),
                                status_code: HyperTextTransferProtocolStatusCode::OK,
                            }
                        })
                    };
                    if response.status_code.is_success() {
                        return (response.status_code, Json(response.response_body))
                            .into_response();
                    }
                    (response.status_code, response.response_body.to_string()).into_response()
                }),
            )
            .with_state(mock_telegram_state);
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("c2a8d5f1");
        let listener_address = listener.local_addr().expect("b7d3e4a9");
        let server_task = tokio::spawn(async move {
            drop(axum::serve(listener, mock_application).await);
        });
        (listener_address, server_task)
    }

    async fn wait_until(
        maximum_attempts: usize,
        sleep_duration: Duration,
        condition: impl Fn() -> bool,
    ) {
        for _attempt in 0..maximum_attempts {
            if condition() {
                return;
            }
            sleep(sleep_duration).await;
        }
    }

    #[tokio::test]
    async fn worker_authorizes_chat_id_and_username_and_deduplicates_update_identifier() {
        let mock_telegram_state = MockTelegramState {
            get_updates_responses: Arc::new(Mutex::new(VecDeque::from([
                MockHyperTextTransferProtocolResponse {
                    response_body: json!({
                        "ok": true,
                        "result": [
                            {
                                "update_id": 100i64,
                                "message": {
                                    "chat": { "id": 222i64 },
                                    "from": { "username": "kuqmua" },
                                    "text": "/health"
                                }
                            },
                            {
                                "update_id": 101i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "from": { "username": "other_user" },
                                    "text": "/health"
                                }
                            },
                            {
                                "update_id": 101i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "from": { "username": "kuqmua" },
                                    "text": "/health"
                                }
                            },
                            {
                                "update_id": 102i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "from": { "username": "kuqmua" },
                                    "text": "/health"
                                }
                            }
                        ]
                    }),
                    status_code: HyperTextTransferProtocolStatusCode::OK,
                },
            ]))),
            ..MockTelegramState::default()
        };
        let (listener_address, server_task) =
            spawn_mock_telegram_server(mock_telegram_state.clone()).await;
        let environment_variables = build_environment(format!("http://{listener_address}"), [
            ("TELEGRAM_CHAT_ID", String::from("111")),
            ("TELEGRAM_ALLOWED_USERNAME", String::from("@kuqmua")),
        ]);
        let runtime_settings = Arc::new(
            ServiceConfiguration::from_environment_map(&environment_variables).expect("e1b9d7c3"),
        );
        let runtime_state = build_runtime_state(&runtime_settings).expect("d4f2a8b6");
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let worker_task = tokio::spawn(run_updates_loop(
            runtime_state.clone(),
            Arc::clone(&runtime_settings),
            shutdown_receiver,
        ));
        wait_until(100, Duration::from_millis(20), || {
            mock_telegram_state
                .sent_message_count
                .load(Ordering::SeqCst)
                >= 1
        })
        .await;
        let _send_result = shutdown_sender.send(true);
        worker_task.await.expect("f6c2a1d8");
        let sent_messages_guard = mock_telegram_state.sent_messages.lock().await;
        assert_eq!(sent_messages_guard.len(), 1);
        drop(sent_messages_guard);
        let rendered_metrics = runtime_state.metrics().render_prometheus(
            runtime_state.is_polling_ready(),
            runtime_state.configured_telegram_chat_identifier(),
        );
        assert!(rendered_metrics.contains("update_duplicates_total 1"));
        server_task.abort();
    }

    #[tokio::test]
    async fn worker_replies_username_required_when_sender_username_is_missing() {
        let mock_telegram_state = MockTelegramState {
            get_updates_responses: Arc::new(Mutex::new(VecDeque::from([
                MockHyperTextTransferProtocolResponse {
                    response_body: json!({
                        "ok": true,
                        "result": [
                            {
                                "update_id": 103i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "text": "/health"
                                }
                            }
                        ]
                    }),
                    status_code: HyperTextTransferProtocolStatusCode::OK,
                },
            ]))),
            ..MockTelegramState::default()
        };
        let (listener_address, server_task) =
            spawn_mock_telegram_server(mock_telegram_state.clone()).await;
        let environment_variables = build_environment(format!("http://{listener_address}"), [
            ("TELEGRAM_CHAT_ID", String::from("111")),
            ("TELEGRAM_ALLOWED_USERNAME", String::from("kuqmua")),
        ]);
        let runtime_settings = Arc::new(
            ServiceConfiguration::from_environment_map(&environment_variables).expect("d8a17f2c"),
        );
        let runtime_state = build_runtime_state(&runtime_settings).expect("e4b79a1d");
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let worker_task = tokio::spawn(run_updates_loop(
            runtime_state,
            Arc::clone(&runtime_settings),
            shutdown_receiver,
        ));
        wait_until(100, Duration::from_millis(20), || {
            mock_telegram_state
                .sent_message_count
                .load(Ordering::SeqCst)
                >= 1
        })
        .await;
        let _send_result = shutdown_sender.send(true);
        worker_task.await.expect("f1c48b7a");
        let sent_messages_guard = mock_telegram_state.sent_messages.lock().await;
        assert_eq!(sent_messages_guard.len(), 1);
        let first_message = sent_messages_guard.first().expect("b2a9f6d4");
        assert!(first_message.contains(SYSTEM_MESSAGE_USERNAME_REQUIRED));
        drop(sent_messages_guard);
        server_task.abort();
    }

    #[tokio::test]
    async fn worker_reports_codex_timeout() {
        let mock_telegram_state = MockTelegramState {
            get_updates_responses: Arc::new(Mutex::new(VecDeque::from([
                MockHyperTextTransferProtocolResponse {
                    response_body: json!({
                        "ok": true,
                        "result": [
                            {
                                "update_id": 404i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "text": "/codex run something"
                                }
                            }
                        ]
                    }),
                    status_code: HyperTextTransferProtocolStatusCode::OK,
                },
            ]))),
            ..MockTelegramState::default()
        };
        let (listener_address, server_task) =
            spawn_mock_telegram_server(mock_telegram_state.clone()).await;
        let random_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0u128, |duration| duration.as_nanos());
        let codex_script_path: PathBuf =
            env::temp_dir().join(format!("codex-timeout-{random_suffix}.sh"));
        let script_body = "\
#!/usr/bin/env bash
if [ \"$1\" = \"login\" ] && [ \"$2\" = \"status\" ]; then
  exit 0
fi
if [ \"$1\" = \"exec\" ]; then
  sleep 2
  echo \"done\"
  exit 0
fi
exit 0
";
        fs::write(&codex_script_path, script_body).expect("d7f2b1a9");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let permissions = fs::Permissions::from_mode(0o755);
            fs::set_permissions(&codex_script_path, permissions).expect("e4a9c2f6");
        };
        let environment_variables = build_environment(format!("http://{listener_address}"), [
            ("TELEGRAM_CHAT_ID", String::from("111")),
            ("CODEX_BINARY_PATH", codex_script_path.to_string_lossy().into_owned()),
            ("CODEX_TIMEOUT_SECONDS", String::from("1")),
        ]);
        let runtime_settings = Arc::new(
            ServiceConfiguration::from_environment_map(&environment_variables).expect("f2a1d8c6"),
        );
        let runtime_state = build_runtime_state(&runtime_settings).expect("a7e9d3b1");
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let worker_task = tokio::spawn(run_updates_loop(
            runtime_state.clone(),
            Arc::clone(&runtime_settings),
            shutdown_receiver,
        ));
        wait_until(250, Duration::from_millis(20), || {
            mock_telegram_state
                .sent_message_count
                .load(Ordering::SeqCst)
                >= 3
        })
        .await;
        let _send_result = shutdown_sender.send(true);
        worker_task.await.expect("c1b7f4d9");
        let rendered_metrics = runtime_state.metrics().render_prometheus(
            runtime_state.is_polling_ready(),
            runtime_state.configured_telegram_chat_identifier(),
        );
        assert!(rendered_metrics.contains("task_timeout_total 1"));
        let sent_messages_guard = mock_telegram_state.sent_messages.lock().await;
        assert!(
            sent_messages_guard
                .iter()
                .any(|message_text| { message_text.contains(SYSTEM_MESSAGE_CODEX_TIMED_OUT) })
        );
        drop(sent_messages_guard);
        let _remove_result = fs::remove_file(codex_script_path);
        server_task.abort();
    }

    #[tokio::test]
    async fn worker_cancels_codex_task() {
        let mock_telegram_state = MockTelegramState {
            get_updates_responses: Arc::new(Mutex::new(VecDeque::from([
                MockHyperTextTransferProtocolResponse {
                    response_body: json!({
                        "ok": true,
                        "result": [
                            {
                                "update_id": 501i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "text": "/codex run first task"
                                }
                            },
                            {
                                "update_id": 502i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "text": "/cancel 1"
                                }
                            }
                        ]
                    }),
                    status_code: HyperTextTransferProtocolStatusCode::OK,
                },
            ]))),
            ..MockTelegramState::default()
        };
        let (listener_address, server_task) =
            spawn_mock_telegram_server(mock_telegram_state.clone()).await;
        let random_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0u128, |duration| duration.as_nanos());
        let codex_script_path: PathBuf =
            env::temp_dir().join(format!("codex-cancel-{random_suffix}.sh"));
        let script_body = "\
#!/usr/bin/env bash
if [ \"$1\" = \"login\" ] && [ \"$2\" = \"status\" ]; then
  exit 0
fi
if [ \"$1\" = \"exec\" ]; then
  sleep 3
  echo \"done\"
  exit 0
fi
exit 0
";
        fs::write(&codex_script_path, script_body).expect("a9e71c4d");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let permissions = fs::Permissions::from_mode(0o755);
            fs::set_permissions(&codex_script_path, permissions).expect("be6f1a23");
        };
        let environment_variables = build_environment(format!("http://{listener_address}"), [
            ("TELEGRAM_CHAT_ID", String::from("111")),
            ("CODEX_BINARY_PATH", codex_script_path.to_string_lossy().into_owned()),
            ("CODEX_TIMEOUT_SECONDS", String::from("20")),
        ]);
        let runtime_settings = Arc::new(
            ServiceConfiguration::from_environment_map(&environment_variables).expect("d4a38f1c"),
        );
        let runtime_state = build_runtime_state(&runtime_settings).expect("e81c27ab");
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let worker_task = tokio::spawn(run_updates_loop(
            runtime_state.clone(),
            Arc::clone(&runtime_settings),
            shutdown_receiver,
        ));
        wait_until(250, Duration::from_millis(20), || {
            mock_telegram_state
                .sent_message_count
                .load(Ordering::SeqCst)
                >= 3
        })
        .await;
        let _send_result = shutdown_sender.send(true);
        worker_task.await.expect("c2f79a5e");
        let sent_messages_guard = mock_telegram_state.sent_messages.lock().await;
        assert!(
            sent_messages_guard
                .iter()
                .any(|message_text| { message_text.contains(SYSTEM_MESSAGE_CODEX_CANCELLED) })
        );
        drop(sent_messages_guard);
        let _remove_result = fs::remove_file(codex_script_path);
        server_task.abort();
    }

    #[tokio::test]
    async fn worker_cancels_task_when_queue_wait_limit_is_exceeded() {
        let mock_telegram_state = MockTelegramState {
            get_updates_responses: Arc::new(Mutex::new(VecDeque::from([
                MockHyperTextTransferProtocolResponse {
                    response_body: json!({
                        "ok": true,
                        "result": [
                            {
                                "update_id": 601i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "text": "/codex run first task"
                                }
                            },
                            {
                                "update_id": 602i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "text": "/codex run second task"
                                }
                            }
                        ]
                    }),
                    status_code: HyperTextTransferProtocolStatusCode::OK,
                },
            ]))),
            ..MockTelegramState::default()
        };
        let (listener_address, server_task) =
            spawn_mock_telegram_server(mock_telegram_state.clone()).await;
        let random_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0u128, |duration| duration.as_nanos());
        let codex_script_path: PathBuf =
            env::temp_dir().join(format!("codex-queue-timeout-{random_suffix}.sh"));
        let script_body = "\
#!/usr/bin/env bash
if [ \"$1\" = \"login\" ] && [ \"$2\" = \"status\" ]; then
  exit 0
fi
if [ \"$1\" = \"exec\" ]; then
  sleep 3
  echo \"done\"
  exit 0
fi
exit 0
";
        fs::write(&codex_script_path, script_body).expect("d8a1b2c3");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let permissions = fs::Permissions::from_mode(0o755);
            fs::set_permissions(&codex_script_path, permissions).expect("f4e5a6b7");
        };
        let environment_variables = build_environment(format!("http://{listener_address}"), [
            ("TELEGRAM_CHAT_ID", String::from("111")),
            ("CODEX_BINARY_PATH", codex_script_path.to_string_lossy().into_owned()),
            ("CODEX_MAX_PARALLEL_TASKS", String::from("1")),
            ("CODEX_TIMEOUT_SECONDS", String::from("20")),
            ("TASK_QUEUE_MAX_WAIT_SECONDS", String::from("1")),
        ]);
        let runtime_settings = Arc::new(
            ServiceConfiguration::from_environment_map(&environment_variables).expect("a1b2c3d4"),
        );
        let runtime_state = build_runtime_state(&runtime_settings).expect("c3d4e5f6");
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let worker_task = tokio::spawn(run_updates_loop(
            runtime_state.clone(),
            Arc::clone(&runtime_settings),
            shutdown_receiver,
        ));
        wait_until(300, Duration::from_millis(20), || {
            mock_telegram_state
                .sent_message_count
                .load(Ordering::SeqCst)
                >= 4
        })
        .await;
        let _send_result = shutdown_sender.send(true);
        worker_task.await.expect("b5c6d7e8");
        let sent_messages_guard = mock_telegram_state.sent_messages.lock().await;
        assert!(sent_messages_guard.iter().any(|message_text| {
            message_text.contains(SYSTEM_MESSAGE_TASK_QUEUE_WAIT_EXCEEDED)
        }));
        drop(sent_messages_guard);
        let _remove_result = fs::remove_file(codex_script_path);
        server_task.abort();
    }

    #[tokio::test]
    async fn worker_retry_creates_new_task_and_reuses_prompt_text() {
        let mock_telegram_state = MockTelegramState {
            get_updates_responses: Arc::new(Mutex::new(VecDeque::from([
                MockHyperTextTransferProtocolResponse {
                    response_body: json!({
                        "ok": true,
                        "result": [
                            {
                                "update_id": 701i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "text": "/codex inherited prompt"
                                }
                            },
                            {
                                "update_id": 702i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "text": "/retry 1"
                                }
                            }
                        ]
                    }),
                    status_code: HyperTextTransferProtocolStatusCode::OK,
                },
            ]))),
            ..MockTelegramState::default()
        };
        let (listener_address, server_task) =
            spawn_mock_telegram_server(mock_telegram_state.clone()).await;
        let random_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0u128, |duration| duration.as_nanos());
        let codex_script_path: PathBuf =
            env::temp_dir().join(format!("codex-retry-{random_suffix}.sh"));
        let script_body = "\
#!/usr/bin/env bash
if [ \"$1\" = \"login\" ] && [ \"$2\" = \"status\" ]; then
  exit 0
fi
if [ \"$1\" = \"exec\" ]; then
  echo \"${!#}\"
  exit 0
fi
exit 0
";
        fs::write(&codex_script_path, script_body).expect("d9a1b2c3");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let permissions = fs::Permissions::from_mode(0o755);
            fs::set_permissions(&codex_script_path, permissions).expect("e1b2c3d4");
        };
        let environment_variables = build_environment(format!("http://{listener_address}"), [
            ("TELEGRAM_CHAT_ID", String::from("111")),
            ("CODEX_BINARY_PATH", codex_script_path.to_string_lossy().into_owned()),
            ("CODEX_TIMEOUT_SECONDS", String::from("20")),
        ]);
        let runtime_settings = Arc::new(
            ServiceConfiguration::from_environment_map(&environment_variables).expect("f1a2b3c4"),
        );
        let runtime_state = build_runtime_state(&runtime_settings).expect("a1b2c3d4");
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let worker_task = tokio::spawn(run_updates_loop(
            runtime_state,
            Arc::clone(&runtime_settings),
            shutdown_receiver,
        ));
        wait_until(300, Duration::from_millis(20), || {
            mock_telegram_state
                .sent_message_count
                .load(Ordering::SeqCst)
                >= 6
        })
        .await;
        let _send_result = shutdown_sender.send(true);
        worker_task.await.expect("b1c2d3e4");
        let sent_messages_guard = mock_telegram_state.sent_messages.lock().await;
        assert!(
            sent_messages_guard
                .iter()
                .any(|message_text| { message_text.contains("Task queued: 1") })
        );
        assert!(
            sent_messages_guard
                .iter()
                .any(|message_text| { message_text.contains("Task queued: 2") })
        );
        assert!(sent_messages_guard.iter().any(|message_text| {
            message_text.contains("Task finished: 2") && message_text.contains("inherited prompt")
        }));
        drop(sent_messages_guard);
        let _remove_result = fs::remove_file(codex_script_path);
        server_task.abort();
    }

    #[tokio::test]
    async fn worker_denies_non_allowed_username_access() {
        let mock_telegram_state = MockTelegramState {
            get_updates_responses: Arc::new(Mutex::new(VecDeque::from([
                MockHyperTextTransferProtocolResponse {
                    response_body: json!({
                        "ok": true,
                        "result": [
                            {
                                "update_id": 711i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "from": { "username": "another_user" },
                                    "text": "/health"
                                }
                            }
                        ]
                    }),
                    status_code: HyperTextTransferProtocolStatusCode::OK,
                },
            ]))),
            ..MockTelegramState::default()
        };
        let (listener_address, server_task) =
            spawn_mock_telegram_server(mock_telegram_state.clone()).await;
        let environment_variables = build_environment(format!("http://{listener_address}"), [
            ("TELEGRAM_CHAT_ID", String::from("111")),
            ("TELEGRAM_ALLOWED_USERNAME", String::from("kuqmua")),
        ]);
        let runtime_settings = Arc::new(
            ServiceConfiguration::from_environment_map(&environment_variables).expect("c1d2e3f4"),
        );
        let runtime_state = build_runtime_state(&runtime_settings).expect("d1e2f3a4");
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let worker_task = tokio::spawn(run_updates_loop(
            runtime_state,
            Arc::clone(&runtime_settings),
            shutdown_receiver,
        ));
        sleep(Duration::from_millis(250)).await;
        let _send_result = shutdown_sender.send(true);
        worker_task.await.expect("e1f2a3b4");
        let sent_messages_guard = mock_telegram_state.sent_messages.lock().await;
        assert_eq!(sent_messages_guard.len(), 0);
        drop(sent_messages_guard);
        server_task.abort();
    }

    #[tokio::test]
    async fn worker_handles_cancel_race_during_execution_start() {
        let mock_telegram_state = MockTelegramState {
            get_updates_responses: Arc::new(Mutex::new(VecDeque::from([
                MockHyperTextTransferProtocolResponse {
                    response_body: json!({
                        "ok": true,
                        "result": [
                            {
                                "update_id": 721i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "text": "/codex race task"
                                }
                            }
                        ]
                    }),
                    status_code: HyperTextTransferProtocolStatusCode::OK,
                },
                MockHyperTextTransferProtocolResponse {
                    response_body: json!({
                        "ok": true,
                        "result": [
                            {
                                "update_id": 722i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "text": "/cancel 1"
                                }
                            }
                        ]
                    }),
                    status_code: HyperTextTransferProtocolStatusCode::OK,
                },
            ]))),
            ..MockTelegramState::default()
        };
        let (listener_address, server_task) =
            spawn_mock_telegram_server(mock_telegram_state.clone()).await;
        let random_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0u128, |duration| duration.as_nanos());
        let codex_script_path: PathBuf =
            env::temp_dir().join(format!("codex-race-cancel-{random_suffix}.sh"));
        let script_body = "\
#!/usr/bin/env bash
if [ \"$1\" = \"login\" ] && [ \"$2\" = \"status\" ]; then
  exit 0
fi
if [ \"$1\" = \"exec\" ]; then
  sleep 2
  echo \"done\"
  exit 0
fi
exit 0
";
        fs::write(&codex_script_path, script_body).expect("f1a3b5c7");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let permissions = fs::Permissions::from_mode(0o755);
            fs::set_permissions(&codex_script_path, permissions).expect("a2b4c6d8");
        };
        let environment_variables = build_environment(format!("http://{listener_address}"), [
            ("TELEGRAM_CHAT_ID", String::from("111")),
            ("CODEX_BINARY_PATH", codex_script_path.to_string_lossy().into_owned()),
            ("CODEX_TIMEOUT_SECONDS", String::from("20")),
        ]);
        let runtime_settings = Arc::new(
            ServiceConfiguration::from_environment_map(&environment_variables).expect("b2c4d6e8"),
        );
        let runtime_state = build_runtime_state(&runtime_settings).expect("c3d5e7f9");
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let worker_task = tokio::spawn(run_updates_loop(
            runtime_state,
            Arc::clone(&runtime_settings),
            shutdown_receiver,
        ));
        wait_until(300, Duration::from_millis(20), || {
            mock_telegram_state
                .sent_message_count
                .load(Ordering::SeqCst)
                >= 3
        })
        .await;
        let _send_result = shutdown_sender.send(true);
        worker_task.await.expect("d4e6f8a1");
        let sent_messages_guard = mock_telegram_state.sent_messages.lock().await;
        assert!(
            sent_messages_guard
                .iter()
                .any(|message_text| { message_text.contains(SYSTEM_MESSAGE_CODEX_CANCELLED) })
        );
        assert!(
            !sent_messages_guard
                .iter()
                .any(|message_text| message_text.contains("Task finished: 1"))
        );
        drop(sent_messages_guard);
        let _remove_result = fs::remove_file(codex_script_path);
        server_task.abort();
    }

    #[tokio::test]
    async fn worker_chunks_large_codex_output_for_telegram() {
        let mock_telegram_state = MockTelegramState {
            get_updates_responses: Arc::new(Mutex::new(VecDeque::from([
                MockHyperTextTransferProtocolResponse {
                    response_body: json!({
                        "ok": true,
                        "result": [
                            {
                                "update_id": 731i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "text": "/codex huge output"
                                }
                            }
                        ]
                    }),
                    status_code: HyperTextTransferProtocolStatusCode::OK,
                },
            ]))),
            ..MockTelegramState::default()
        };
        let (listener_address, server_task) =
            spawn_mock_telegram_server(mock_telegram_state.clone()).await;
        let random_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0u128, |duration| duration.as_nanos());
        let codex_script_path: PathBuf =
            env::temp_dir().join(format!("codex-huge-output-{random_suffix}.sh"));
        let script_body = "\
#!/usr/bin/env bash
if [ \"$1\" = \"login\" ] && [ \"$2\" = \"status\" ]; then
  exit 0
fi
if [ \"$1\" = \"exec\" ]; then
  for i in {1..700}; do
    printf \"x\"
  done
  printf \"\\n\"
  exit 0
fi
exit 0
";
        fs::write(&codex_script_path, script_body).expect("a7b8c9d1");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let permissions = fs::Permissions::from_mode(0o755);
            fs::set_permissions(&codex_script_path, permissions).expect("b8c9d1e2");
        };
        let environment_variables = build_environment(format!("http://{listener_address}"), [
            ("TELEGRAM_CHAT_ID", String::from("111")),
            ("CODEX_BINARY_PATH", codex_script_path.to_string_lossy().into_owned()),
            ("TELEGRAM_MESSAGE_MAX_CHARACTERS", String::from("120")),
        ]);
        let runtime_settings = Arc::new(
            ServiceConfiguration::from_environment_map(&environment_variables).expect("c9d1e2f3"),
        );
        let runtime_state = build_runtime_state(&runtime_settings).expect("d1e2f3a4");
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let worker_task = tokio::spawn(run_updates_loop(
            runtime_state,
            Arc::clone(&runtime_settings),
            shutdown_receiver,
        ));
        wait_until(400, Duration::from_millis(20), || {
            mock_telegram_state
                .sent_message_count
                .load(Ordering::SeqCst)
                >= 4
        })
        .await;
        let _send_result = shutdown_sender.send(true);
        worker_task.await.expect("e2f3a4b5");
        let sent_message_count = mock_telegram_state
            .sent_message_count
            .load(Ordering::SeqCst);
        assert!(sent_message_count > 3);
        let sent_messages_guard = mock_telegram_state.sent_messages.lock().await;
        assert!(
            sent_messages_guard
                .iter()
                .any(|message_text| message_text.contains("Task finished: 1"))
        );
        let combined_messages = sent_messages_guard.join("");
        assert!(combined_messages.contains("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"));
        drop(sent_messages_guard);
        let _remove_result = fs::remove_file(codex_script_path);
        server_task.abort();
    }

    #[tokio::test]
    async fn worker_streams_codex_process_output_for_codex_process_command() {
        let mock_telegram_state = MockTelegramState {
            get_updates_responses: Arc::new(Mutex::new(VecDeque::from([
                MockHyperTextTransferProtocolResponse {
                    response_body: json!({
                        "ok": true,
                        "result": [
                            {
                                "update_id": 735i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "text": "/codex_process show process"
                                }
                            }
                        ]
                    }),
                    status_code: HyperTextTransferProtocolStatusCode::OK,
                },
            ]))),
            ..MockTelegramState::default()
        };
        let (listener_address, server_task) =
            spawn_mock_telegram_server(mock_telegram_state.clone()).await;
        let random_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0u128, |duration| duration.as_nanos());
        let codex_script_path: PathBuf =
            env::temp_dir().join(format!("codex-process-output-{random_suffix}.sh"));
        let script_body = "\
#!/usr/bin/env bash
if [ \"$1\" = \"login\" ] && [ \"$2\" = \"status\" ]; then
  exit 0
fi
if [ \"$1\" = \"exec\" ] && [ \"$2\" = \"--skip-git-repo-check\" ] && [ \"$3\" = \"--json\" ]; then
  printf '{\"event\":\"task.started\"}\\n'
  sleep 1
  printf '{\"event\":\"task.completed\"}\\n'
  exit 0
fi
exit 0
";
        fs::write(&codex_script_path, script_body).expect("d2e3f4a5");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let permissions = fs::Permissions::from_mode(0o755);
            fs::set_permissions(&codex_script_path, permissions).expect("e3f4a5b6");
        };
        let environment_variables = build_environment(format!("http://{listener_address}"), [
            ("TELEGRAM_CHAT_ID", String::from("111")),
            ("CODEX_BINARY_PATH", codex_script_path.to_string_lossy().into_owned()),
            ("CODEX_TIMEOUT_SECONDS", String::from("20")),
        ]);
        let runtime_settings = Arc::new(
            ServiceConfiguration::from_environment_map(&environment_variables).expect("f4a5b6c7"),
        );
        let runtime_state = build_runtime_state(&runtime_settings).expect("a5b6c7d8");
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let worker_task = tokio::spawn(run_updates_loop(
            runtime_state,
            Arc::clone(&runtime_settings),
            shutdown_receiver,
        ));
        wait_until(400, Duration::from_millis(20), || {
            mock_telegram_state
                .sent_message_count
                .load(Ordering::SeqCst)
                >= 4
        })
        .await;
        let _send_result = shutdown_sender.send(true);
        worker_task.await.expect("b6c7d8e9");
        let sent_messages_guard = mock_telegram_state.sent_messages.lock().await;
        assert!(sent_messages_guard.iter().any(|message_text| {
            message_text.contains("Codex process output")
                && message_text.contains("\"event\":\"task.started\"")
        }));
        assert!(sent_messages_guard.iter().any(|message_text| {
            message_text.contains("Task finished: 1")
                && message_text.contains("\"event\":\"task.completed\"")
        }));
        drop(sent_messages_guard);
        let _remove_result = fs::remove_file(codex_script_path);
        server_task.abort();
    }

    #[tokio::test]
    async fn worker_executes_openai_command_and_returns_response() {
        let mock_telegram_state = MockTelegramState {
            openai_chat_completions_responses: Arc::new(Mutex::new(VecDeque::from([
                MockHyperTextTransferProtocolResponse {
                    response_body: json!({
                        "choices": [
                            {
                                "message": {
                                    "content": "openai completed response"
                                }
                            }
                        ]
                    }),
                    status_code: HyperTextTransferProtocolStatusCode::OK,
                },
            ]))),
            get_updates_responses: Arc::new(Mutex::new(VecDeque::from([
                MockHyperTextTransferProtocolResponse {
                    response_body: json!({
                            "ok": true,
                            "result": [
                                {
                                "update_id": 739i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                "text": "/openai --configuration 1 explain ownership"
                                }
                            }
                        ]
                    }),
                    status_code: HyperTextTransferProtocolStatusCode::OK,
                },
            ]))),
            ..MockTelegramState::default()
        };
        let (listener_address, server_task) =
            spawn_mock_telegram_server(mock_telegram_state.clone()).await;
        let environment_variables = build_environment(format!("http://{listener_address}"), [
            ("TELEGRAM_CHAT_ID", String::from("111")),
            (
                "OPENAI_CONFIGURATIONS",
                format!(
                    "[{{\"api_key\":\"test-openai-key\",\"api_url\":\"http://{listener_address}/openai/chat/completions\",\"model\":\"gpt-4o-mini\"}}]"
                ),
            ),
        ]);
        let runtime_settings = Arc::new(
            ServiceConfiguration::from_environment_map(&environment_variables).expect("e5b2f7a1"),
        );
        let runtime_state = build_runtime_state(&runtime_settings).expect("a2d8c4f9");
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let worker_task = tokio::spawn(run_updates_loop(
            runtime_state,
            Arc::clone(&runtime_settings),
            shutdown_receiver,
        ));
        wait_until(250, Duration::from_millis(20), || {
            mock_telegram_state
                .sent_message_count
                .load(Ordering::SeqCst)
                >= 1
                && mock_telegram_state
                    .openai_request_count
                    .load(Ordering::SeqCst)
                    >= 1
        })
        .await;
        let _send_result = shutdown_sender.send(true);
        worker_task.await.expect("b8d4f1a6");
        let sent_messages_guard = mock_telegram_state.sent_messages.lock().await;
        assert!(
            sent_messages_guard
                .iter()
                .any(|message_text| { message_text.contains("openai completed response") })
        );
        drop(sent_messages_guard);
        server_task.abort();
    }

    #[tokio::test]
    async fn worker_returns_openai_uniform_resource_locators_from_configuration() {
        let mock_telegram_state = MockTelegramState {
            get_updates_responses: Arc::new(Mutex::new(VecDeque::from([
                MockHyperTextTransferProtocolResponse {
                    response_body: json!({
                        "ok": true,
                        "result": [
                            {
                                "update_id": 740i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "text": "/openai_urls"
                                }
                            }
                        ]
                    }),
                    status_code: HyperTextTransferProtocolStatusCode::OK,
                },
            ]))),
            ..MockTelegramState::default()
        };
        let (listener_address, server_task) =
            spawn_mock_telegram_server(mock_telegram_state.clone()).await;
        let environment_variables = build_environment(format!("http://{listener_address}"), [
            ("TELEGRAM_CHAT_ID", String::from("111")),
            (
                "OPENAI_CONFIGURATIONS",
                String::from(
                    "[{\"api_key\":\"key-1\",\"api_url\":\"http://127.0.0.1:9100/chat/completions\",\"model\":\"gpt-4o-mini\"},{\"api_key\":\"key-2\",\"api_url\":\"http://127.0.0.1:9200/chat/completions\",\"model\":\"gpt-4.1\"}]",
                ),
            ),
        ]);
        let runtime_settings = Arc::new(
            ServiceConfiguration::from_environment_map(&environment_variables).expect("b9d2a4f6"),
        );
        let runtime_state = build_runtime_state(&runtime_settings).expect("c8f1a5d7");
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let worker_task = tokio::spawn(run_updates_loop(
            runtime_state,
            Arc::clone(&runtime_settings),
            shutdown_receiver,
        ));
        wait_until(250, Duration::from_millis(20), || {
            mock_telegram_state
                .sent_message_count
                .load(Ordering::SeqCst)
                >= 1
        })
        .await;
        let _send_result = shutdown_sender.send(true);
        worker_task.await.expect("d7a3c1e5");
        let sent_messages_guard = mock_telegram_state.sent_messages.lock().await;
        assert!(sent_messages_guard.iter().any(|message_text| {
            message_text.contains("openai_api_urls:")
                && message_text.contains("1. http://127.0.0.1:9100/chat/completions")
                && message_text.contains("2. http://127.0.0.1:9200/chat/completions")
        }));
        drop(sent_messages_guard);
        server_task.abort();
    }

    #[tokio::test]
    async fn worker_enforces_rate_limit_per_user_within_one_minute() {
        let mock_telegram_state = MockTelegramState {
            get_updates_responses: Arc::new(Mutex::new(VecDeque::from([
                MockHyperTextTransferProtocolResponse {
                    response_body: json!({
                        "ok": true,
                        "result": [
                            {
                                "update_id": 741i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "from": { "username": "kuqmua" },
                                    "text": "/codex first"
                                }
                            },
                            {
                                "update_id": 742i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "from": { "username": "kuqmua" },
                                    "text": "/codex second"
                                }
                            }
                        ]
                    }),
                    status_code: HyperTextTransferProtocolStatusCode::OK,
                },
            ]))),
            ..MockTelegramState::default()
        };
        let (listener_address, server_task) =
            spawn_mock_telegram_server(mock_telegram_state.clone()).await;
        let random_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0u128, |duration| duration.as_nanos());
        let codex_script_path: PathBuf =
            env::temp_dir().join(format!("codex-rate-limit-{random_suffix}.sh"));
        let script_body = "\
#!/usr/bin/env bash
if [ \"$1\" = \"login\" ] && [ \"$2\" = \"status\" ]; then
  exit 0
fi
if [ \"$1\" = \"exec\" ]; then
  sleep 1
  echo \"done\"
  exit 0
fi
exit 0
";
        fs::write(&codex_script_path, script_body).expect("f3a4b5c6");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let permissions = fs::Permissions::from_mode(0o755);
            fs::set_permissions(&codex_script_path, permissions).expect("a4b5c6d7");
        };
        let environment_variables = build_environment(format!("http://{listener_address}"), [
            ("TELEGRAM_CHAT_ID", String::from("111")),
            ("TELEGRAM_ALLOWED_USERNAME", String::from("kuqmua")),
            ("CODEX_BINARY_PATH", codex_script_path.to_string_lossy().into_owned()),
            ("TASK_RATE_LIMIT_PER_MINUTE", String::from("1")),
        ]);
        let runtime_settings = Arc::new(
            ServiceConfiguration::from_environment_map(&environment_variables).expect("b5c6d7e8"),
        );
        let runtime_state = build_runtime_state(&runtime_settings).expect("c6d7e8f9");
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let worker_task = tokio::spawn(run_updates_loop(
            runtime_state,
            Arc::clone(&runtime_settings),
            shutdown_receiver,
        ));
        wait_until(250, Duration::from_millis(20), || {
            mock_telegram_state
                .sent_message_count
                .load(Ordering::SeqCst)
                >= 2
        })
        .await;
        let _send_result = shutdown_sender.send(true);
        worker_task.await.expect("d7e8f9a1");
        let sent_messages_guard = mock_telegram_state.sent_messages.lock().await;
        assert!(
            sent_messages_guard
                .iter()
                .any(|message_text| { message_text.contains("Task queued: 1") })
        );
        assert!(
            sent_messages_guard
                .iter()
                .any(|message_text| { message_text.contains(SYSTEM_MESSAGE_TASK_RATE_LIMITED) })
        );
        drop(sent_messages_guard);
        let _remove_result = fs::remove_file(codex_script_path);
        server_task.abort();
    }

    #[tokio::test]
    async fn worker_executes_codex_cli_commands_for_sandbox_debug_features_and_selected_subcommands()
     {
        let mock_telegram_state = MockTelegramState {
            get_updates_responses: Arc::new(Mutex::new(VecDeque::from([
                MockHyperTextTransferProtocolResponse {
                    response_body: json!({
                        "ok": true,
                        "result": [
                            {
                                "update_id": 751i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "text": "/sandbox linux run"
                                }
                            },
                            {
                                "update_id": 752i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "text": "/debug app-server send-message-v2 ping"
                                }
                            },
                            {
                                "update_id": 753i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "text": "/features list"
                                }
                            },
                            {
                                "update_id": 754i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "text": "/mcp_list"
                                }
                            },
                            {
                                "update_id": 755i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "text": "/debug_prompt_input summarize context"
                                }
                            },
                            {
                                "update_id": 756i64,
                                "message": {
                                    "chat": { "id": 111i64 },
                                    "text": "/features_list"
                                }
                            }
                        ]
                    }),
                    status_code: HyperTextTransferProtocolStatusCode::OK,
                },
            ]))),
            ..MockTelegramState::default()
        };
        let (listener_address, server_task) =
            spawn_mock_telegram_server(mock_telegram_state.clone()).await;
        let random_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0u128, |duration| duration.as_nanos());
        let codex_script_path: PathBuf =
            env::temp_dir().join(format!("codex-cli-commands-{random_suffix}.sh"));
        let script_body = "\
#!/usr/bin/env bash
if [ \"$1\" = \"login\" ] && [ \"$2\" = \"status\" ]; then
  exit 0
fi
if [ \"$1\" = \"sandbox\" ]; then
  echo \"sandbox:$*\"
  exit 0
fi
if [ \"$1\" = \"debug\" ] && [ \"$2\" = \"app-server\" ]; then
  echo \"debug_app_server:$*\"
  exit 0
fi
if [ \"$1\" = \"debug\" ] && [ \"$2\" = \"prompt-input\" ]; then
  echo \"debug_prompt_input:$3\"
  exit 0
fi
if [ \"$1\" = \"features\" ] && [ \"$2\" = \"list\" ]; then
  echo \"features_list:$*\"
  exit 0
fi
if [ \"$1\" = \"mcp\" ] && [ \"$2\" = \"list\" ]; then
  echo \"mcp_list:$*\"
  exit 0
fi
echo \"unexpected:$*\"
exit 1
";
        fs::write(&codex_script_path, script_body).expect("e8c1a7d5");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let permissions = fs::Permissions::from_mode(0o755);
            fs::set_permissions(&codex_script_path, permissions).expect("a1f3c9d7");
        };
        let environment_variables = build_environment(format!("http://{listener_address}"), [
            ("TELEGRAM_CHAT_ID", String::from("111")),
            ("CODEX_BINARY_PATH", codex_script_path.to_string_lossy().into_owned()),
            ("CODEX_TIMEOUT_SECONDS", String::from("20")),
        ]);
        let runtime_settings = Arc::new(
            ServiceConfiguration::from_environment_map(&environment_variables).expect("b9d3e4a2"),
        );
        let runtime_state = build_runtime_state(&runtime_settings).expect("c7f5a1d8");
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let worker_task = tokio::spawn(run_updates_loop(
            runtime_state,
            Arc::clone(&runtime_settings),
            shutdown_receiver,
        ));
        wait_until(400, Duration::from_millis(20), || {
            mock_telegram_state
                .sent_message_count
                .load(Ordering::SeqCst)
                >= 6
        })
        .await;
        let _send_result = shutdown_sender.send(true);
        worker_task.await.expect("d4e6a8b1");
        let sent_messages_guard = mock_telegram_state.sent_messages.lock().await;
        assert!(
            sent_messages_guard
                .iter()
                .any(|message_text| { message_text.contains("sandbox:sandbox linux run") })
        );
        assert!(sent_messages_guard.iter().any(|message_text| {
            message_text.contains("debug_app_server:debug app-server send-message-v2 ping")
        }));
        assert!(
            sent_messages_guard
                .iter()
                .any(|message_text| { message_text.contains("mcp_list:mcp list") })
        );
        assert!(
            sent_messages_guard.iter().any(|message_text| {
                message_text.contains("debug_prompt_input:summarize context")
            })
        );
        assert!(
            sent_messages_guard
                .iter()
                .any(|message_text| { message_text.contains("features_list:features list") })
        );
        drop(sent_messages_guard);
        let _remove_result = fs::remove_file(codex_script_path);
        server_task.abort();
    }
}
