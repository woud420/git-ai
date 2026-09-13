use super::*;

pub(super) struct ScopedEnvVar {
    pub(super) key: &'static str,
    pub(super) previous: Option<std::ffi::OsString>,
}

impl ScopedEnvVar {
    pub(super) fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }
}

impl Drop for ScopedEnvVar {
    fn drop(&mut self) {
        unsafe {
            if let Some(previous) = self.previous.as_ref() {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}

pub(super) struct MockApiServer {
    pub(super) base_url: String,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) rx: mpsc::Receiver<Value>,
    pub(super) thread: Option<thread::JoinHandle<()>>,
}

impl MockApiServer {
    pub(super) fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind mock API server");
        listener
            .set_nonblocking(true)
            .expect("failed to set nonblocking listener");
        let addr = listener.local_addr().expect("failed to read listener addr");
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);

        let thread = thread::spawn(move || {
            while !stop_thread.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        handle_http_connection(stream, &tx);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("mock API accept failed: {}", error),
                }
            }
        });

        Self {
            base_url: format!("http://{}", addr),
            stop,
            rx,
            thread: Some(thread),
        }
    }

    pub(super) fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Collect all requests captured by the mock so far.
    pub(super) fn collect_requests(&mut self) -> Vec<Value> {
        let mut requests = Vec::new();
        while let Ok(request) = self.rx.try_recv() {
            requests.push(request);
        }
        requests
    }
}

impl Drop for MockApiServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.base_url.trim_start_matches("http://"));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub(super) fn handle_http_connection(mut stream: TcpStream, tx: &mpsc::Sender<Value>) {
    let Some((path, body)) = read_http_request(&mut stream) else {
        return;
    };

    let request_json: Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!({}));

    let response_body = match path.as_str() {
        "/worker/cas/upload" => {
            let _ = tx.send(json!({ "path": path, "body": request_json }));
            let hashes = request_json["objects"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|object| object["hash"].as_str().map(|hash| hash.to_string()))
                .collect::<Vec<_>>();
            json!({
                "results": hashes.iter().map(|hash| {
                    json!({
                        "hash": hash,
                        "status": "ok"
                    })
                }).collect::<Vec<_>>(),
                "success_count": hashes.len(),
                "failure_count": 0
            })
            .to_string()
        }
        "/worker/metrics/upload" => {
            let _ = tx.send(json!({ "path": path, "body": request_json }));
            json!({ "errors": [] }).to_string()
        }
        "/worker/logs/upload" => {
            let accepted = request_json["events"].as_array().map_or(0, Vec::len);
            let _ = tx.send(json!({ "path": path, "body": request_json }));
            json!({
                "accepted": accepted,
                "dropped": 0,
                "enqueued": true,
                "errors": []
            })
            .to_string()
        }
        "/worker/notes/upload" => {
            let _ = tx.send(json!({ "path": path, "body": request_json }));
            let success_count = request_json["entries"]
                .as_array()
                .map(|entries| entries.len())
                .unwrap_or(0);
            json!({
                "success_count": success_count,
                "failure_count": 0
            })
            .to_string()
        }
        _ => "{}".to_string(),
    };

    write_http_response(&mut stream, response_body.as_bytes());
}

pub(super) fn read_http_request(stream: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    // Accepted sockets inherit the listener's nonblocking mode on macOS.
    // This parser must wait for client bytes instead of treating WouldBlock as EOF.
    stream
        .set_nonblocking(false)
        .expect("failed to make mock API request reads blocking");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("failed to set mock API read timeout");

    let mut buffer = Vec::new();
    let header_end = loop {
        let mut chunk = [0u8; 4096];
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(end) = find_header_end(&buffer) {
            break end;
        }
    };

    let headers = String::from_utf8_lossy(&buffer[..header_end]);
    let request_line = headers.lines().next()?;
    let path = request_line.split_whitespace().nth(1)?.to_string();
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
        })
        .unwrap_or(0);

    while buffer.len() - header_end < content_length {
        let mut chunk = [0u8; 4096];
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }

    Some((
        path,
        buffer[header_end..header_end + content_length].to_vec(),
    ))
}

pub(super) fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|idx| idx + 4)
}

pub(super) fn write_http_response(stream: &mut TcpStream, body: &[u8]) {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .expect("failed to write mock API response headers");
    stream
        .write_all(body)
        .expect("failed to write mock API response body");
    stream.flush().expect("failed to flush mock API response");
}

pub(super) fn claude_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("example-claude-code.jsonl")
}

pub(super) fn assert_post_commit_uploads_prompt_cas() {
    let mock_api = MockApiServer::start();
    let _api_base_url = ScopedEnvVar::set("GIT_AI_API_BASE_URL", mock_api.base_url());
    let _api_key = ScopedEnvVar::set("GIT_AI_API_KEY", "test-api-key");

    // These tests depend on per-test API env vars being visible to the daemon.
    // A shared daemon may already be running from an earlier test with different env.
    let mut repo = TestRepo::new_with_daemon_scope(DaemonTestScope::Dedicated);
    repo.patch_git_ai_config(|patch| {
        patch.exclude_prompts_in_repositories = Some(vec![]);
        patch.prompt_storage = Some("default".to_string());
        patch.telemetry_oss_disabled = Some(true);
    });

    let repo_root = repo.canonical_path();
    let file_path = repo_root.join("test.ts");
    fs::write(&file_path, "const x = 1;\n").expect("failed to write initial file");
    repo.stage_all_and_commit("Initial commit")
        .expect("initial commit should succeed");

    let transcript_path = repo_root.join("claude-session.jsonl");
    fs::copy(claude_fixture_path(), &transcript_path).expect("failed to copy transcript fixture");

    let hook_input = json!({
        "cwd": repo_root.to_string_lossy().to_string(),
        "hook_event_name": "PostToolUse",
        "transcript_path": transcript_path.to_string_lossy().to_string(),
        "tool_input": {
            "file_path": file_path.to_string_lossy().to_string()
        }
    })
    .to_string();

    fs::write(&file_path, "const x = 1;\n// ai line one\n").expect("failed to write AI edit");
    repo.git_ai(&["checkpoint", "claude", "--hook-input", &hook_input])
        .expect("checkpoint should succeed");

    let commit = repo
        .stage_all_and_commit("Add AI line")
        .expect("AI commit should succeed");

    // Sessions no longer upload messages to CAS - only prompts do.
    // Since claude checkpoints create sessions, not prompts, we don't expect a CAS upload.
    // Verify that the authorship note is created with a session record.
    let note = repo
        .read_authorship_note(&commit.commit_sha)
        .expect("commit should have authorship note");
    let log =
        git_ai::model::authorship_log_serialization::AuthorshipLog::deserialize_from_string(&note)
            .expect("authorship note should deserialize");
    // AI checkpoints now produce sessions (not prompts)
    let _session = log
        .metadata
        .sessions
        .values()
        .next()
        .expect("authorship note should contain one session");
    // Sessions no longer have messages or messages_url fields
}
