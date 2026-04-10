use dotenvy as _;
use reqwest as _;
use serde as _;
use serde_json as _;
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
        http::StatusCode,
        response::{IntoResponse as _, Response},
        routing::{get, post},
    };
    use serde_json::{Value, json};
    use server::{
        build_runtime_state,
        settings::ServiceConfiguration,
        shared::{
            SYSTEM_MESSAGE_CODEX_CANCELLED, SYSTEM_MESSAGE_CODEX_TIMED_OUT,
            SYSTEM_MESSAGE_TASK_QUEUE_WAIT_EXCEEDED, SYSTEM_MESSAGE_USERNAME_REQUIRED,
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
    struct MockHttpResponse {
        response_body: Value,
        status_code: StatusCode,
    }

    #[derive(Clone, Default)]
    struct MockTelegramState {
        get_updates_call_count: Arc<AtomicUsize>,
        get_updates_responses: Arc<Mutex<VecDeque<MockHttpResponse>>>,
        send_message_responses: Arc<Mutex<VecDeque<MockHttpResponse>>>,
        sent_message_count: Arc<AtomicUsize>,
        sent_messages: Arc<Mutex<Vec<String>>>,
    }

    fn build_environment(
        telegram_api_base_url: String,
        additional_environment: impl IntoIterator<Item = (&'static str, String)>,
    ) -> BTreeMap<String, String> {
        let mut environment_variables = BTreeMap::from([
            (
                String::from("TELEGRAM_BOT_TOKEN"),
                String::from("123456789:ABCDEFGHIJKLMNOPQRSTUVWXYZ"),
            ),
            (String::from("HOST"), String::from("127.0.0.1")),
            (String::from("PORT"), String::from("8080")),
            (String::from("TELEGRAM_API_BASE_URL"), telegram_api_base_url),
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
                            response_guard
                                .pop_front()
                                .unwrap_or_else(|| MockHttpResponse {
                                    response_body: json!({
                                        "ok": true,
                                        "result": []
                                    }),
                                    status_code: StatusCode::OK,
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
                            response_guard
                                .pop_front()
                                .unwrap_or_else(|| MockHttpResponse {
                                    response_body: json!({
                                        "ok": true,
                                        "result": {}
                                    }),
                                    status_code: StatusCode::OK,
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
            get_updates_responses: Arc::new(Mutex::new(VecDeque::from([MockHttpResponse {
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
                status_code: StatusCode::OK,
            }]))),
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
            get_updates_responses: Arc::new(Mutex::new(VecDeque::from([MockHttpResponse {
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
                status_code: StatusCode::OK,
            }]))),
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
            get_updates_responses: Arc::new(Mutex::new(VecDeque::from([MockHttpResponse {
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
                status_code: StatusCode::OK,
            }]))),
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
            get_updates_responses: Arc::new(Mutex::new(VecDeque::from([MockHttpResponse {
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
                status_code: StatusCode::OK,
            }]))),
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
            get_updates_responses: Arc::new(Mutex::new(VecDeque::from([MockHttpResponse {
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
                status_code: StatusCode::OK,
            }]))),
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
}
