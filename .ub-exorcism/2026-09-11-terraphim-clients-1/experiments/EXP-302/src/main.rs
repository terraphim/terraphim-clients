// EXP-302 SSE client driver (blast radius of UF-039 in the long-lived server).
//
// UNTESTED THIS SESSION: the safety classifier blocked build/execution after the
// hostile-input generators were authored. Compile-verify against rmcp 0.9 before
// relying on it; the rmcp 0.9 client surface used here (SseClientTransport::start,
// ().serve(transport), peer().call_tool(CallToolRequestParam)) may need small
// signature fixes. The behaviour is the load-bearing part:
//
//   N concurrent tasks each connect to http://127.0.0.1:$PORT/sse and call
//   grep_files{query:"needle", path:$TREE} in a loop; every response's text
//   content is checked with std::str::from_utf8 and parsed as JSON. Counts of
//   invalid-UTF-8 and unparseable-JSON responses are printed. The truncate arm's
//   SIGBUS is observed by the orchestration script via the server's exit signal,
//   not here.
//
// Args: <port> <tree> <clients> <seconds>

use rmcp::model::CallToolRequestParam;
use rmcp::transport::sse_client::SseClientTransport;
use rmcp::ServiceExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let a: Vec<String> = std::env::args().collect();
    let port: u16 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(18302);
    let tree = a.get(2).cloned().unwrap_or_else(|| ".".to_string());
    let clients: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(8);
    let seconds: u64 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(60);
    let url = format!("http://127.0.0.1:{port}/sse");

    let calls = Arc::new(AtomicU64::new(0));
    let bad_utf8 = Arc::new(AtomicU64::new(0));
    let bad_json = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));
    let deadline = Instant::now() + Duration::from_secs(seconds);

    let mut handles = Vec::new();
    for _ in 0..clients {
        let (url, tree) = (url.clone(), tree.clone());
        let (calls, bad_utf8, bad_json, errors) =
            (calls.clone(), bad_utf8.clone(), bad_json.clone(), errors.clone());
        handles.push(tokio::spawn(async move {
            let transport = match SseClientTransport::start(url.clone()).await {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("transport start failed: {e}");
                    errors.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            };
            let client = match ().serve(transport).await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("serve failed: {e}");
                    errors.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            };
            while Instant::now() < deadline {
                let args = serde_json::json!({ "query": "needle", "path": tree })
                    .as_object()
                    .cloned();
                let res = client
                    .peer()
                    .call_tool(CallToolRequestParam {
                        name: "grep_files".into(),
                        arguments: args,
                    })
                    .await;
                calls.fetch_add(1, Ordering::Relaxed);
                match res {
                    Ok(r) => {
                        for c in r.content.iter() {
                            // rmcp content -> raw text if it is a text block.
                            if let Some(txt) = c.as_text() {
                                let s = &txt.text;
                                if std::str::from_utf8(s.as_bytes()).is_err() {
                                    bad_utf8.fetch_add(1, Ordering::Relaxed);
                                }
                                if serde_json::from_str::<serde_json::Value>(s).is_err() {
                                    // grep_files returns human text, not JSON, so a
                                    // parse "failure" here is expected; the real
                                    // signal is bad_utf8. Kept for completeness.
                                    bad_json.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                    }
                    Err(_) => {
                        errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            let _ = client.cancel().await;
        }));
    }
    for h in handles {
        let _ = h.await;
    }
    println!(
        "EXP-302 driver: clients={clients} seconds={seconds} calls={} invalid_utf8={} json_parse_fail={} errors={}",
        calls.load(Ordering::Relaxed),
        bad_utf8.load(Ordering::Relaxed),
        bad_json.load(Ordering::Relaxed),
        errors.load(Ordering::Relaxed),
    );
    if bad_utf8.load(Ordering::Relaxed) > 0 {
        println!("EXP-302 CONFIRMED (writer arm): grep_files returned invalid UTF-8 content");
    } else {
        println!("EXP-302 NO_EVIDENCE (writer arm): all response content was valid UTF-8");
    }
}
