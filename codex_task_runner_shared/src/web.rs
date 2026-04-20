#![allow(clippy::single_call_fn)]

use std::{
    fs,
    io::{ErrorKind, Read as _, Write as _, stdout},
    net::{Shutdown, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

pub(crate) struct LogViewer {
    handle: thread::JoinHandle<Result<(), String>>,
    stop: Arc<AtomicBool>,
}

impl LogViewer {
    pub(crate) fn stop(self) -> Result<(), String> {
        self.stop.store(true, Ordering::Relaxed);
        self.handle
            .join()
            .map_err(|_error| String::from("5e0b2d4f log viewer server thread panicked"))?
    }
}

fn escape_html(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        if ch == '&' {
            out.push_str("&amp;");
        } else if ch == '<' {
            out.push_str("&lt;");
        } else if ch == '>' {
            out.push_str("&gt;");
        } else if ch == '"' {
            out.push_str("&quot;");
        } else {
            out.push(ch);
        }
    }
    out
}

fn list_log_entries(cdx_dir: &Path) -> Vec<(String, PathBuf)> {
    let mut log_entries = Vec::<(String, PathBuf)>::new();
    let read_dir_result = fs::read_dir(cdx_dir);
    if let Ok(dir_entries) = read_dir_result {
        for entry in dir_entries {
            let dir_entry = match entry {
                Ok(value) => value,
                Err(_error) => continue,
            };
            let path = dir_entry.path();
            let is_log = path.extension().and_then(|value| value.to_str()) == Some("log");
            if !is_log {
                continue;
            }
            let file_name = path
                .file_name()
                .map_or_else(String::new, |value| value.to_string_lossy().into_owned());
            if file_name.is_empty() {
                continue;
            }
            log_entries.push((file_name, path));
        }
    }
    log_entries.sort_by(|left, right| left.0.cmp(&right.0));
    log_entries
}

fn selected_idx_from_request(request_buffer: &[u8]) -> Option<usize> {
    let request_text = String::from_utf8_lossy(request_buffer);
    let request_line = request_text.lines().next().unwrap_or("");
    let mut request_line_parts = request_line.split_whitespace();
    let _method = request_line_parts.next()?;
    let request_target = request_line_parts.next()?;
    let query = request_target
        .split_once('?')
        .map_or("", |(_path, query_part)| query_part);
    for pair in query.split('&') {
        let (key, raw_val) = pair.split_once('=').map_or((pair, ""), |parts| parts);
        let Some(decoded_key) = decode_query_component(key) else {
            continue;
        };
        if decoded_key == "i" {
            let Some(decoded_value) = decode_query_component(raw_val) else {
                continue;
            };
            if let Ok(parsed_idx) = decoded_value.parse::<usize>() {
                return Some(parsed_idx);
            }
        }
    }
    None
}
fn request_method_from_request(request_buffer: &[u8]) -> Option<String> {
    let request_text = String::from_utf8_lossy(request_buffer);
    let request_line = request_text.lines().next().unwrap_or("");
    let mut request_line_parts = request_line.split_whitespace();
    let method = request_line_parts.next()?;
    let _target = request_line_parts.next()?;
    Some(method.to_owned())
}
fn decode_query_component(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::<u8>::with_capacity(bytes.len());
    let mut idx = 0usize;
    while idx < bytes.len() {
        let byte = *bytes.get(idx)?;
        if byte == b'+' {
            out.push(b' ');
            idx = idx.saturating_add(1usize);
            continue;
        }
        if byte == b'%' {
            let hi = *bytes.get(idx.saturating_add(1usize))?;
            let lo = *bytes.get(idx.saturating_add(2usize))?;
            let hi_val = hex_value(hi)?;
            let lo_val = hex_value(lo)?;
            out.push((hi_val << 4u8) | lo_val);
            idx = idx.saturating_add(3usize);
            continue;
        }
        out.push(byte);
        idx = idx.saturating_add(1usize);
    }
    String::from_utf8(out).ok()
}
fn write_simple_response(
    stream: &mut TcpStream,
    status_line: &str,
    body: &str,
    include_body: bool,
) -> Result<(), String> {
    let body_bytes = body.as_bytes();
    let headers = format!(
        "{status_line}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: \
         {}\r\nConnection: close\r\n\r\n",
        body_bytes.len()
    );
    stream
        .write_all(headers.as_bytes())
        .map_err(|error| format!("8e2b4c6d failed to write simple response headers: {error}"))?;
    if include_body {
        stream
            .write_all(body_bytes)
            .map_err(|error| format!("9f3c5d7e failed to write simple response body: {error}"))?;
    }
    Ok(())
}
fn hex_value(byte: u8) -> Option<u8> {
    if byte.is_ascii_digit() {
        return Some(byte.saturating_sub(b'0'));
    }
    if (b'a'..=b'f').contains(&byte) {
        return Some(byte.saturating_sub(b'a').saturating_add(10u8));
    }
    if (b'A'..=b'F').contains(&byte) {
        return Some(byte.saturating_sub(b'A').saturating_add(10u8));
    }
    None
}

fn read_request_buffer(stream: &mut TcpStream) -> Result<(Vec<u8>, bool), String> {
    const MAX_REQUEST_BYTES: usize = 8192usize;
    const READ_CHUNK_BYTES: usize = 512usize;
    let mut request = Vec::<u8>::with_capacity(READ_CHUNK_BYTES);
    let mut chunk = [0u8; READ_CHUNK_BYTES];
    loop {
        let read_count = match stream.read(&mut chunk) {
            Ok(value) => value,
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                break;
            }
            Err(error) => {
                return Err(format!("1a7c9e2d failed to read http request: {error}"));
            }
        };
        if read_count == 0usize {
            break;
        }
        let chunk_part = chunk
            .get(..read_count)
            .ok_or_else(|| String::from("2b8d0f3e failed to split request read chunk"))?;
        request.extend_from_slice(chunk_part);
        let has_headers_end = request.windows(4usize).any(|window| window == b"\r\n\r\n");
        if has_headers_end {
            break;
        }
        if request.len() >= MAX_REQUEST_BYTES {
            return Ok((request, true));
        }
    }
    Ok((request, false))
}

fn render_page(log_entries: &[(String, PathBuf)], selected_idx: usize) -> String {
    let mut page = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"refresh\" \
         content=\"2\"><title>cdx_cli \
         logs</title><style>body{font-family:monospace;margin:16px}pre{white-space:pre-wrap;\
         border:1px solid #ccc;padding:10px;background:#fafafa}h1,h2{margin:0 0 12px \
         0}h2{margin-top:18px}select{font-family:monospace}</style></head><body><h1>cdx_cli \
         logs</h1>",
    );
    if log_entries.is_empty() {
        page.push_str("<p>No log files yet in ./cdx_cli_manage</p>");
        page.push_str("</body></html>");
        return page;
    }
    page.push_str(
        "<form method=\"get\"><label for=\"log-select\">log file:</label> <select \
         id=\"log-select\" name=\"i\" onchange=\"this.form.submit()\">",
    );
    let effective_idx = if selected_idx < log_entries.len() {
        selected_idx
    } else {
        0usize
    };
    let mut idx = 0usize;
    while let Some((file_name, _path)) = log_entries.get(idx) {
        let selected_attr = if idx == effective_idx {
            " selected"
        } else {
            ""
        };
        let file_name_html = escape_html(file_name.as_str());
        page.push_str(
            format!("<option value=\"{idx}\"{selected_attr}>{file_name_html}</option>").as_str(),
        );
        idx = idx.saturating_add(1usize);
    }
    page.push_str("</select></form>");
    let Some((selected_file_name, selected_path)) = log_entries.get(effective_idx) else {
        page.push_str("</body></html>");
        return page;
    };
    let content = fs::read(selected_path.as_path()).map_or_else(
        |_source| String::from("[failed to read log file]"),
        |bytes| String::from_utf8_lossy(bytes.as_slice()).into_owned(),
    );
    let selected_name_html = escape_html(selected_file_name.as_str());
    let content_html = escape_html(content.as_str());
    page.push_str(format!("<h2>{selected_name_html}</h2><pre>{content_html}</pre>").as_str());
    page.push_str("</body></html>");
    page
}

pub(crate) fn start_log_viewer(server_addr: &str, cdx_dir: &Path) -> Result<LogViewer, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_copy = Arc::clone(&stop);
    let cdx_dir_copy = cdx_dir.to_path_buf();
    let server_addr_copy = server_addr.to_owned();
    writeln!(stdout().lock(), "log_view_server_start_attempt: http://{server_addr_copy}")
        .map_err(|error| format!("7b3d9f1a failed to print log viewer start attempt: {error}"))?;
    let listener = TcpListener::bind(server_addr_copy.as_str()).map_err(|error| {
        format!("6f1a3b5c failed to bind log viewer server on `{server_addr_copy}`: {error}")
    })?;
    writeln!(stdout().lock(), "log_view_server_started: http://{server_addr_copy}")
        .map_err(|error| format!("6a2c8d1f failed to print log viewer server started: {error}"))?;
    listener.set_nonblocking(true).map_err(|error| {
        format!("2b7e9a1c failed to set nonblocking mode for log viewer server: {error}")
    })?;
    let handle = thread::spawn(move || -> Result<(), String> {
        while !stop_copy.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _addr)) => {
                    if stream
                        .set_read_timeout(Some(Duration::from_millis(500u64)))
                        .is_err()
                    {
                        drop(stream.shutdown(Shutdown::Both));
                        continue;
                    }
                    if stream
                        .set_write_timeout(Some(Duration::from_millis(500u64)))
                        .is_err()
                    {
                        drop(stream.shutdown(Shutdown::Both));
                        continue;
                    }
                    let (request_buffer, request_too_large) = match read_request_buffer(&mut stream)
                    {
                        Ok(value) => value,
                        Err(_error) => continue,
                    };
                    if request_too_large {
                        drop(write_simple_response(
                            &mut stream,
                            "HTTP/1.1 413 Payload Too Large",
                            "request too large\n",
                            true,
                        ));
                        drop(stream.shutdown(Shutdown::Both));
                        continue;
                    }
                    let request_method = request_method_from_request(request_buffer.as_slice());
                    if request_method.is_none() {
                        drop(write_simple_response(
                            &mut stream,
                            "HTTP/1.1 400 Bad Request",
                            "bad request\n",
                            true,
                        ));
                        drop(stream.shutdown(Shutdown::Both));
                        continue;
                    }
                    if !matches!(request_method.as_deref(), Some("GET" | "HEAD")) {
                        drop(write_simple_response(
                            &mut stream,
                            "HTTP/1.1 405 Method Not Allowed",
                            "method not allowed\n",
                            true,
                        ));
                        drop(stream.shutdown(Shutdown::Both));
                        continue;
                    }
                    let is_head = matches!(request_method.as_deref(), Some("HEAD"));
                    let log_entries = list_log_entries(cdx_dir_copy.as_path());
                    let selected_idx = selected_idx_from_request(&request_buffer)
                        .filter(|idx| *idx < log_entries.len())
                        .unwrap_or(0usize);
                    let page = render_page(log_entries.as_slice(), selected_idx);
                    let body = page.into_bytes();
                    let headers = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; \
                         charset=utf-8\r\nContent-Length: {}\r\nCache-Control: \
                         no-cache\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    if stream.write_all(headers.as_bytes()).is_err() {
                        drop(stream.shutdown(Shutdown::Both));
                        continue;
                    }
                    if !is_head && stream.write_all(body.as_slice()).is_err() {
                        drop(stream.shutdown(Shutdown::Both));
                    }
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(200u64));
                }
                Err(error) => {
                    return Err(format!("4a8c1e3d log viewer accept failed: {error}"));
                }
            }
        }
        Ok(())
    });
    Ok(LogViewer { handle, stop })
}

#[cfg(test)]
mod tests {
    use std::{
        env::temp_dir,
        fs::{create_dir_all, remove_dir, remove_file, write},
        path::PathBuf,
        process::id,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{render_page, request_method_from_request, selected_idx_from_request};

    fn request_buffer(request_line: &str) -> Vec<u8> {
        let mut out = vec![0u8; 1024usize];
        let raw = format!("{request_line}\r\nHost: localhost\r\n\r\n");
        let bytes = raw.as_bytes();
        let len = bytes.len().min(out.len());
        let out_prefix = out
            .get_mut(..len)
            .unwrap_or_else(|| panic!("6e0a2b4d failed to get mutable request buffer prefix"));
        let bytes_prefix = bytes
            .get(..len)
            .unwrap_or_else(|| panic!("7f1b3c5e failed to get immutable request bytes prefix"));
        out_prefix.copy_from_slice(bytes_prefix);
        out
    }

    #[test]
    fn selected_idx_from_request_parses_valid_query_index() {
        let buffer = request_buffer("GET /?i=7 HTTP/1.1");
        let parsed = selected_idx_from_request(&buffer);
        assert_eq!(parsed, Some(7usize));
    }

    #[test]
    fn selected_idx_from_request_returns_none_without_i_query() {
        let buffer = request_buffer("GET / HTTP/1.1");
        let parsed = selected_idx_from_request(&buffer);
        assert_eq!(parsed, None);
    }

    #[test]
    fn selected_idx_from_request_returns_none_for_non_numeric_i() {
        let buffer = request_buffer("GET /?i=abc HTTP/1.1");
        let parsed = selected_idx_from_request(&buffer);
        assert_eq!(parsed, None);
    }

    #[test]
    fn selected_idx_from_request_parses_i_when_it_is_not_first_param() {
        let buffer = request_buffer("GET /?x=1&i=3&y=2 HTTP/1.1");
        let parsed = selected_idx_from_request(&buffer);
        assert_eq!(parsed, Some(3usize));
    }
    #[test]
    fn selected_idx_from_request_parses_percent_encoded_index() {
        let buffer = request_buffer("GET /?i=%33 HTTP/1.1");
        let parsed = selected_idx_from_request(&buffer);
        assert_eq!(parsed, Some(3usize));
    }
    #[test]
    fn selected_idx_from_request_rejects_malformed_percent_encoding() {
        let buffer = request_buffer("GET /?i=%3 HTTP/1.1");
        let parsed = selected_idx_from_request(&buffer);
        assert_eq!(parsed, None);
    }
    #[test]
    fn selected_idx_from_request_ignores_invalid_non_target_param_and_parses_i() {
        let buffer = request_buffer("GET /?x=%zz&i=4 HTTP/1.1");
        let parsed = selected_idx_from_request(&buffer);
        assert_eq!(parsed, Some(4usize));
    }
    #[test]
    fn selected_idx_from_request_skips_invalid_i_before_valid_i() {
        let buffer = request_buffer("GET /?i=%zz&i=4 HTTP/1.1");
        let parsed = selected_idx_from_request(&buffer);
        assert_eq!(parsed, Some(4usize));
    }
    #[test]
    fn selected_idx_from_request_returns_none_when_target_is_missing() {
        let buffer = request_buffer("GET");
        let parsed = selected_idx_from_request(&buffer);
        assert_eq!(parsed, None);
    }
    #[test]
    fn request_method_from_request_parses_get() {
        let buffer = request_buffer("GET / HTTP/1.1");
        let parsed = request_method_from_request(&buffer);
        assert_eq!(parsed.as_deref(), Some("GET"));
    }
    #[test]
    fn request_method_from_request_parses_head() {
        let buffer = request_buffer("HEAD / HTTP/1.1");
        let parsed = request_method_from_request(&buffer);
        assert_eq!(parsed.as_deref(), Some("HEAD"));
    }
    #[test]
    fn request_method_from_request_returns_none_for_empty_buffer() {
        let parsed = request_method_from_request(&[]);
        assert_eq!(parsed, None);
    }
    #[test]
    fn request_method_from_request_returns_none_without_target() {
        let buffer = request_buffer("GET");
        let parsed = request_method_from_request(&buffer);
        assert_eq!(parsed, None);
    }

    #[test]
    fn render_page_displays_non_utf8_log_as_lossy_text() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0u128, |value| value.as_nanos());
        let root_dir = temp_dir().join(format!("cdx_cli_test_non_utf8_log_{}_{}", id(), stamp));
        create_dir_all(root_dir.as_path())
            .unwrap_or_else(|error| panic!("2e6a8c0d failed to create temp dir: {error}"));
        let log_path = root_dir.join("x_cdx_cli.log");
        write(log_path.as_path(), [0x48u8, 0x69u8, 0xffu8, 0x0au8])
            .unwrap_or_else(|error| panic!("3f7b9d1e failed to write non-utf8 log: {error}"));
        let page = render_page(
            &[(String::from("x_cdx_cli.log"), PathBuf::from(log_path.as_path()))],
            0usize,
        );
        assert!(page.contains("Hi"));
        assert!(page.contains("\u{fffd}"));
        drop(remove_file(log_path.as_path()));
        drop(remove_dir(root_dir.as_path()));
    }

    #[test]
    fn render_page_falls_back_to_first_entry_when_selected_idx_out_of_range() {
        let page = render_page(
            &[
                (String::from("a.log"), PathBuf::from("a.log")),
                (String::from("b.log"), PathBuf::from("b.log")),
            ],
            99usize,
        );
        assert!(page.contains("<option value=\"0\" selected>a.log</option>"));
    }
}
