// nex-cf Mock Simulation Server
//
// 로컬에서 nex-cf의 전체 플로우를 시뮬레이션하는 HTTP 서버입니다.
// R2 대신 in-memory HashMap을 사용하여 CF 인프라 없이도
// 문서 수집 → Fact 저장 → Semantic 검색까지의 end-to-end 파이프라인을 검증합니다.
//
// Usage:
//   cargo run -p nex-cf-mock
//   curl http://localhost:8080/
//   curl "http://localhost:8080/ingest?text=Hello+world+semantic+search&origin=test"
//   curl "http://localhost:8080/search?q=semantic"
//   curl "http://localhost:8080/state"

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use nex::io::{BatchIo, FileIo, IoFuture, WriteOp};

// ── MockBucket: in-memory HashMap mimicking R2 ──────────────────────────

#[derive(Clone)]
pub struct MockBucket {
    data: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MockBucket {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.data.lock().unwrap().get(key).cloned()
    }

    pub fn put(&self, key: &str, value: &[u8]) {
        self.data
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_vec());
    }

    pub fn delete(&self, key: &str) {
        self.data.lock().unwrap().remove(key);
    }

    pub fn list(&self, prefix: &str) -> Vec<String> {
        self.data
            .lock()
            .unwrap()
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect()
    }
}

impl Default for MockBucket {
    fn default() -> Self {
        Self::new()
    }
}

// ── MockIo: FileIo + BatchIo over MockBucket ────────────────────────────

pub struct MockIo {
    bucket: MockBucket,
}

impl MockIo {
    pub fn new(bucket: MockBucket) -> Self {
        Self { bucket }
    }
}

impl FileIo for MockIo {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        let b = self.bucket.clone();
        let p = path.to_string();
        Box::pin(async move { Ok(b.get(&p)) })
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        let b = self.bucket.clone();
        let p = path.to_string();
        let d = data.to_vec();
        Box::pin(async move {
            b.put(&p, &d);
            Ok(())
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        let b = self.bucket.clone();
        let p = prefix.to_string();
        Box::pin(async move { Ok(b.list(&p)) })
    }

    fn delete<'a>(&'a self, path: &'a str) -> IoFuture<'a, ()> {
        let b = self.bucket.clone();
        let p = path.to_string();
        Box::pin(async move {
            b.delete(&p);
            Ok(())
        })
    }
}

impl BatchIo for MockIo {
    fn apply_batch<'a>(&'a self, ops: &'a [WriteOp]) -> IoFuture<'a, ()> {
        let b = self.bucket.clone();
        let items: Vec<(String, Vec<u8>, bool)> = ops
            .iter()
            .map(|op| match op {
                WriteOp::Write { path, data } => (path.clone(), data.clone(), false),
                WriteOp::Delete { path } => (path.clone(), Vec::new(), true),
            })
            .collect();
        Box::pin(async move {
            for (path, data, is_del) in items {
                if is_del {
                    b.delete(&path);
                } else {
                    b.put(&path, &data);
                }
            }
            Ok(())
        })
    }
}

// ── MockClock ───────────────────────────────────────────────────────────

pub struct MockClock {
    now_nanos: u64,
    now_secs: u64,
}

impl MockClock {
    pub fn new(nanos: u64) -> Self {
        Self {
            now_nanos: nanos,
            now_secs: nanos / 1_000_000_000,
        }
    }
}

impl nexus_model::Now for MockClock {
    fn now_nanos(&self) -> u64 {
        self.now_nanos
    }
    fn now_secs(&self) -> u64 {
        self.now_secs
    }
}

// ── Semantic Stores ────────────────────────────────────────────────────

use nexus_gateway_nex_cf::stores::bm25::InMemoryBm25;

// ── TextQuery ────────────────────────

// ── HTTP server ─────────────────────────────────────────────────────────

async fn handle_client(mut stream: TcpStream, storage: &nex::FihStorage<MockIo>) {
    let (reader, mut writer) = stream.split();
    let mut buf_reader = BufReader::new(reader);
    let mut request_line = String::new();

    if buf_reader.read_line(&mut request_line).await.is_err() {
        return;
    }

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        let _ = writer
            .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
            .await;
        return;
    }

    let _method = parts[0];
    let path_with_query = parts[1];

    // Read headers until blank line
    loop {
        let mut header = String::new();
        if buf_reader.read_line(&mut header).await.is_err() || header.trim().is_empty() {
            break;
        }
    }

    let (path, query_map) = parse_path(path_with_query);

    let response = match path.as_str() {
        "/" | "/fact" | "/intent" | "/claim" | "/conclude" | "/state" | "/flush" | "/rebuild" => {
            let q_vec: Vec<(String, String)> = query_map
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            // Reuse the generic handler from nexus-gateway-nex-cf
            let (code, content_type, body) =
                nexus_gateway_nex_cf::handle_path(storage, &path, &q_vec).await;
            http_response(code, &content_type, &body)
        }

        "/ingest" => {
            let text = query_map.get("text").cloned().unwrap_or_default();
            let origin = query_map
                .get("origin")
                .cloned()
                .unwrap_or_else(|| "ingest".into());
            if text.is_empty() {
                http_response(
                    400,
                    "application/json",
                    r#"{"error":"missing 'text' parameter"}"#,
                )
            } else {
                match nexus_gateway_nex_cf::ingest_document(storage, &text, &origin).await {
                    Ok(id) => http_response(
                        200,
                        "application/json",
                        &serde_json::json!({"status":"ingested","id": id}).to_string(),
                    ),
                    Err(e) => http_response(
                        500,
                        "application/json",
                        &serde_json::json!({"error": e}).to_string(),
                    ),
                }
            }
        }

        "/search" => {
            let q = query_map.get("q").cloned().unwrap_or_default();
            if q.is_empty() {
                http_response(
                    400,
                    "application/json",
                    r#"{"error":"missing 'q' parameter"}"#,
                )
            } else {
                let query = nexus_gateway_nex_cf::cf_io::TextQuery { text: q };
                match storage.semantic_search(&query, 10).await {
                    Ok(results) => {
                        let items: Vec<serde_json::Value> = results
                            .iter()
                            .map(|(idx, score)| {
                                serde_json::json!({
                                    "index": idx,
                                    "score": score,
                                    "id": storage.resolve_semantic_idx(*idx),
                                })
                            })
                            .collect();
                        http_response(
                            200,
                            "application/json",
                            &serde_json::json!({"results": items}).to_string(),
                        )
                    }
                    Err(e) => http_response(
                        500,
                        "application/json",
                        &serde_json::json!({"error": format!("search: {e}")}).to_string(),
                    ),
                }
            }
        }

        _ => http_response(404, "application/json", r#"{"error":"not found"}"#),
    };

    let _ = writer.write_all(response.as_bytes()).await;
    let _ = writer.flush().await;
}

fn parse_path(path_with_query: &str) -> (String, HashMap<String, String>) {
    if let Some(pos) = path_with_query.find('?') {
        let path = path_with_query[..pos].to_string();
        let mut params = HashMap::new();
        for pair in path_with_query[pos + 1..].split('&') {
            if let Some(eq) = pair.find('=') {
                let k = url_decode(&pair[..eq]);
                let v = url_decode(&pair[eq + 1..]);
                params.insert(k, v);
            }
        }
        (path, params)
    } else {
        (path_with_query.to_string(), HashMap::new())
    }
}

fn url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hi = chars.next().and_then(|c| c.to_digit(16)).unwrap_or(0) as u8;
            let lo = chars.next().and_then(|c| c.to_digit(16)).unwrap_or(0) as u8;
            result.push((hi * 16 + lo) as char);
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}

fn http_response(code: u16, content_type: &str, body: &str) -> String {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        409 => "Conflict",
        500 => "Internal Server Error",
        _ => "Unknown",
    };
    format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

// ── Main ────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let bucket = MockBucket::new();
    let io = MockIo::new(bucket.clone());
    let storage = nex::FihStorage::with_clock(
        io,
        "nex-cf-mock",
        Box::new(MockClock::new(1_700_000_000_000_000_000)),
    );

    // Register semantic stores
    storage.register_semantic_store(Box::new(InMemoryBm25::new()));

    println!("─── nex-cf Mock Simulation Server ───");
    println!("Listening on http://localhost:8080");
    println!("Endpoints:");
    println!("  GET  /             — service info");
    println!("  GET  /state        — read board state");
    println!("  POST /fact         — submit fact (?id, ?origin, ?content, ?creator)");
    println!("  POST /intent       — submit intent (?id, ?from, ?desc, ?creator)");
    println!("  POST /claim        — claim intent (?id, ?agent)");
    println!("  POST /conclude     — conclude intent (?id, ?result)");
    println!("  POST /flush        — flush to mock R2");
    println!("  POST /rebuild      — rebuild from mock R2");
    println!("  POST /ingest       — ingest document (?text, ?origin)");
    println!("  GET  /search       — semantic search (?q)");
    println!("Try: curl 'http://localhost:8080/ingest?text=Hello+semantic+world&origin=test'");
    println!("     curl 'http://localhost:8080/search?q=semantic'");
    println!("────────────────────────────────────────");

    let listener = TcpListener::bind("127.0.0.1:8080").await.unwrap();

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                // FihStorage uses RefCell internally (not Send/Sync),
                // so we process requests sequentially on a single thread.
                // This is fine for a local simulation server.
                handle_client(stream, &storage).await;
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
}
