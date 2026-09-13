//! EXP-012: non-ASCII input through byte-index string-slicing sites.
//! Real calls: Session::summary (model.rs:326-336, also reached via search_sessions ->
//! session_to_document) and SessionEnricher::extract_context (enricher.rs:177-188,
//! reached via enrich_session; Aho-Corasick positions are byte offsets).
//! Mechanism copies (private items): the `&t[..40]` display truncation idiom used at
//! repl/handler.rs:1995 and siblings, and find_paragraph_end + its caller slice at
//! commands/markdown_parser.rs:419-470 (feature repl-custom, private).
use std::panic::{catch_unwind, AssertUnwindSafe};
use terraphim_sessions::model::{Message, MessageRole, Session, SessionMetadata};
use terraphim_sessions::search::search_sessions;
use terraphim_sessions::enrichment::SessionEnricher;
use terraphim_types::{NormalizedTerm, NormalizedTermValue, Thesaurus};

fn user_session(content: &str) -> Session {
    Session { id: "s1".into(), source: "exp012".into(), external_id: "e1".into(), title: None,
        source_path: std::path::PathBuf::from("/dev/null"), started_at: None, ended_at: None,
        messages: vec![Message::text(0, MessageRole::User, content)], metadata: SessionMetadata::default() }
}
fn msg(e: Box<dyn std::any::Any + Send>) -> String {
    e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()
}
fn report(arm: &str, r: Result<String, Box<dyn std::any::Any + Send>>) -> bool {
    match r { Ok(s) => { println!("{arm}: no panic ({s})"); false }
              Err(e) => { println!("{arm}: PANIC: {}", msg(e)); true } }
}
// handler.rs:1995 idiom, copied verbatim (guard on byte len, slice at fixed byte offset)
fn display_cut(t: &str) -> String { if t.len() > 40 { format!("{}...", &t[..40]) } else { t.to_string() } }
// markdown_parser.rs:452-470 copied verbatim (char-indexed walk, byte-indexed result)
fn find_paragraph_end(content: &str, pos: usize) -> usize {
    let mut end = pos; let content_len = content.len();
    while end < content_len {
        let current_char = content.chars().nth(end).unwrap_or('\0');
        if current_char == '\n' && end + 1 < content_len {
            let next_char = content.chars().nth(end + 1).unwrap_or('\0');
            if next_char == '\n' { end += 2; break; }
        }
        end += 1;
    }
    end.min(content_len)
}
// markdown_parser.rs:419-424 caller idiom (paragraph_start = 0 for a match at offset 0)
fn paragraph_slice(content: &str, start: usize) -> String {
    let paragraph_start = 0usize;
    let paragraph_end = find_paragraph_end(content, start + 20);
    if paragraph_start < paragraph_end && paragraph_start < content.len() {
        content[paragraph_start..paragraph_end].trim().to_string()
    } else { String::new() }
}

fn main() {
    let mut hits = 0;
    // model.rs:332: byte 100 is a continuation byte when the text is "a" + 50 x U+00E9.
    let odd = format!("a{}", "\u{e9}".repeat(50));
    let s = user_session(&odd);
    hits += report("summary arm (Session::summary direct)", catch_unwind(AssertUnwindSafe(|| s.summary().unwrap_or_default()))) as u32;
    hits += report("summary arm (via search_sessions)", catch_unwind(AssertUnwindSafe(|| format!("{} results", search_sessions(&[s.clone()], "x").len())))) as u32;
    hits += report("display arm (handler.rs:1995 idiom)", catch_unwind(AssertUnwindSafe(|| display_cut(&odd)))) as u32;
    // enricher: 40 x U+00E9 (80 bytes) + "a" + "terraphim" => match at byte 81, ctx_start = 31 (inside an e-acute).
    let mut th = Thesaurus::new("exp012".to_string());
    th.insert(NormalizedTermValue::new("terraphim".to_string()), NormalizedTerm::new(1, NormalizedTermValue::new("terraphim".to_string())));
    let text = format!("{}aterraphim", "\u{e9}".repeat(40));
    let es = user_session(&text);
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
    let enricher = SessionEnricher::new(th);
    hits += report("enricher arm (enrich_session -> extract_context)", catch_unwind(AssertUnwindSafe(|| {
        rt.block_on(async { enricher.enrich_session(&es).await.map(|r| format!("{} concepts", r.concepts.concepts.len())).unwrap_or_else(|e| format!("Err {e}")) })
    }))) as u32;
    // paragraph finder: "terraphim " + 11 x U+00E9 + "\n\n" + "tail"; expected byte 23 not a boundary.
    let para = format!("terraphim {}\n\ntail", "\u{e9}".repeat(11));
    hits += report("paragraph arm (markdown_parser idiom)", catch_unwind(AssertUnwindSafe(|| paragraph_slice(&para, 0)))) as u32;
    // control: ASCII of the same byte lengths through every arm returns cleanly
    let ascii = "a".repeat(101);
    let ok = catch_unwind(AssertUnwindSafe(|| { let _ = user_session(&ascii).summary(); let _ = display_cut(&ascii); paragraph_slice("terraphim aaaaaaaaaaa\n\ntail", 0) })).is_ok();
    println!("control (ASCII): {}", if ok { "no panic" } else { "PANIC (unexpected)" });
    println!("EXP-012 RESULT: panicking arms = {hits} of 5");
    println!("EXP-012 {}", if hits > 0 { "PANIC CONFIRMED (safe-Rust panic, not UB)" } else { "NO_EVIDENCE" });
}
