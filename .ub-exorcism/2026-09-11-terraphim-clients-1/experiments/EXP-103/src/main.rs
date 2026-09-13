// EXP-103 (UF-038): an empty pattern string accepted by the session-analyzer
// loader makes the Aho-Corasick matcher emit zero-width matches at non-char
// boundaries, and `&text[start..end]` at matcher.rs:143 panics.
// Real-call arm through the crate's own loader and matcher; control arm with
// the same TOML minus the empty string.
use std::panic::{catch_unwind, AssertUnwindSafe};
use terraphim_session_analyzer::patterns::loader::load_patterns_from_str;
use terraphim_session_analyzer::patterns::matcher::{AhoCorasickMatcher, PatternMatcher};

const TOML_EMPTY: &str = "[[tools]]\nname = \"cargo\"\npatterns = [\"cargo build\", \"\"]\n\
[tools.metadata]\ncategory = \"build\"\nconfidence = 0.9\n";
const TOML_CONTROL: &str = "[[tools]]\nname = \"cargo\"\npatterns = [\"cargo build\"]\n\
[tools.metadata]\ncategory = \"build\"\nconfidence = 0.9\n";
const TOML_WS: &str = "[[tools]]\nname = \"cargo\"\npatterns = [\"cargo build\", \" \"]\n\
[tools.metadata]\ncategory = \"build\"\nconfidence = 0.9\n";

fn run(label: &str, toml: &str, text: &str) -> bool {
    let patterns = match load_patterns_from_str(toml) {
        Ok(p) => p,
        Err(e) => {
            println!("[{label}] loader REJECTED the TOML: {e}");
            return false;
        }
    };
    println!(
        "[{label}] loader accepted {} tool(s), patterns={:?}",
        patterns.len(),
        patterns[0].patterns
    );
    let mut m = AhoCorasickMatcher::new();
    match m.initialize(&patterns) {
        Ok(()) => println!("[{label}] automaton built"),
        Err(e) => {
            println!("[{label}] automaton build FAILED: {e}");
            return false;
        }
    }
    let r = catch_unwind(AssertUnwindSafe(|| m.find_matches(text)));
    match r {
        Err(payload) => {
            let msg = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "<non-string panic payload>".into());
            println!("[{label}] PANIC in find_matches: {msg}");
            true
        }
        Ok(v) => {
            let bad = v
                .iter()
                .filter(|x| !text.is_char_boundary(x.start) || !text.is_char_boundary(x.end))
                .count();
            println!(
                "[{label}] no panic: {} matches, {} at non-boundaries, first 6 = {:?}",
                v.len(),
                bad,
                v.iter().take(6).map(|x| (x.start, x.end, x.text)).collect::<Vec<_>>()
            );
            false
        }
    }
}

fn main() {
    let text = "caf\u{e9} \u{1F600} cargo build";
    let ascii = "cafe :) cargo build";
    println!("EXP-103 text bytes={} chars={}", text.len(), text.chars().count());
    let p1 = run("empty-pattern non-ASCII", TOML_EMPTY, text);
    let p2 = run("empty-pattern ASCII", TOML_EMPTY, ascii);
    let p3 = run("whitespace-pattern non-ASCII", TOML_WS, text);
    let p4 = run("control non-ASCII", TOML_CONTROL, text);
    println!(
        "EXP-103 summary: empty/nonascii panic={p1} empty/ascii panic={p2} whitespace/nonascii panic={p3} control panic={p4}"
    );
    if p1 && !p4 {
        println!("EXP-103 CONFIRMED (panic at matcher.rs:143 with an empty pattern; control clean)");
    } else {
        println!("EXP-103 NO_EVIDENCE (panic not reproduced)");
    }
}
