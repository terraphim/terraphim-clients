// EXP-202 (UF-041): hostile YAML frontmatter into serde_yaml 0.9.34 ->
// unsafe-libyaml 0.2.11 under Miri. Three workspace target shapes are
// replicated: CommandDefinition (commands/mod.rs:102, parsed at
// markdown_parser.rs:143), the injector's Frontmatter (injector.rs:240,
// parsed at :248) and LearningFrontmatter (markdown_store.rs:71, parsed at
// :348). Every case is also parsed as serde_yaml::Value (the most permissive
// consumer). A safe Err or a caught panic is a robustness note; only a Miri
// UB report is the signal.
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct CommandParameter { name: String, #[serde(default)] description: String, #[serde(default)] required: bool }
#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct ResourceLimits { #[serde(default)] max_memory_mb: Option<u64>, #[serde(default)] max_cpu_seconds: Option<u64> }
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
enum ExecutionMode { #[default] Any, Local, Remote }
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
enum RiskLevel { #[default] Low, Medium, High }
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CommandDefinition {
    name: String,
    description: String,
    usage: Option<String>,
    #[serde(default)] parameters: Vec<CommandParameter>,
    #[serde(default)] execution_mode: ExecutionMode,
    #[serde(default)] permissions: Vec<String>,
    #[serde(default)] knowledge_graph_required: Vec<String>,
    category: Option<String>,
    #[serde(default = "default_version")] version: String,
    namespace: Option<String>,
    #[serde(default)] aliases: Vec<String>,
    #[serde(default)] risk_level: RiskLevel,
    #[serde(default)] timeout: Option<u64>,
    #[serde(default)] resource_limits: Option<ResourceLimits>,
}
fn default_version() -> String { "1.0.0".into() }

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct InjectorFrontmatter { id: String, title: String, agent_id: String, trust_level: String, source: String }

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct QualityMetrics { #[serde(default)] score: f64, #[serde(default)] uses: u32 }
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct LearningFrontmatter {
    id: String, title: String, agent_id: String,
    #[serde(default)] captured_at: Option<String>,
    #[serde(default)] updated_at: Option<String>,
    #[serde(default)] promoted_at: Option<String>,
    trust_level: String, source: String,
    #[serde(default)] applicable_agents: Vec<String>,
    #[serde(default)] keywords: Vec<String>,
    #[serde(default)] verify_pattern: Option<String>,
    #[serde(default)] quality: Option<QualityMetrics>,
}

fn corpus() -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<(String, Vec<u8>)> = Vec::new();
    let mut add = |n: &str, s: Vec<u8>| v.push((n.to_string(), s));
    add("control", b"id: a\ntitle: t\nagent_id: x\ntrust_level: L1\nsource: BashHook\nname: n\ndescription: d\n".to_vec());
    add("alias-fanout", b"a: &a [x, y]\nb: &b [*a, *a, *a, *a]\nc: &c [*b, *b, *b, *b]\nd: &d [*c, *c, *c, *c]\nkeywords: &e [*d, *d, *d, *d]\ntitle: *e\n".to_vec());
    add("alias-undefined", b"title: *nope\nid: *nope\n".to_vec());
    add("alias-self", b"title: &a [*a]\n".to_vec());
    add("merge-key-bomb", b"base: &b {x: 1, y: 2}\nm1: &m1 {<<: *b, z: 3}\nm2: &m2 {<<: [*m1, *b], w: 4}\ntitle: {<<: [*m2, *m1, *b]}\n".to_vec());
    add("deep-flow-seq-300", format!("keywords: {}{}", "[".repeat(300), "]".repeat(300)).into_bytes());
    add("deep-flow-map-300", format!("title: {}x{}", "{a: ".repeat(300), "}".repeat(300)).into_bytes());
    add("deep-block-300", { let mut s = String::new(); for i in 0..300 { s.push_str(&" ".repeat(i)); s.push_str("k:\n"); } s.push_str(&" ".repeat(300)); s.push_str("v\n"); s.into_bytes() });
    add("deep-block-seq-300", { let mut s = String::new(); for i in 0..300 { s.push_str(&" ".repeat(i)); s.push_str("- \n"); } s.into_bytes() });
    add("lone-surrogate", b"title: \"\\uD800\"\n".to_vec());
    add("surrogate-pair-split", b"title: \"\\uD83D\\uDE00\"\n".to_vec());
    add("bad-escape", b"title: \"\\q\"\n".to_vec());
    add("truncated-escape-eof", b"title: \"abc\\".to_vec());
    add("truncated-quote-eof", b"title: \"abc".to_vec());
    add("truncated-unicode-escape", b"title: \"\\U0000".to_vec());
    add("invalid-utf8-plain", b"title: \xff\xfe\xc3\ntrust_level: \xf0\x9f\n".to_vec());
    add("invalid-utf8-block-scalar", b"title: |\n  line \xff\xff\xff\xff one\n  \xed\xa0\x80 two\n".to_vec());
    add("invalid-utf8-in-quoted", b"title: \"a\xc0\xafb\"\n".to_vec());
    add("utf16-bom", b"\xff\xfet\x00i\x00t\x00l\x00e\x00:\x00 \x00a\x00\n\x00".to_vec());
    add("utf8-bom-tabs", b"\xef\xbb\xbftitle:\ta\n\t- b\nid: \"x\"\n".to_vec());
    add("nul-bytes", b"title: a\x00b\nid: \x00\n".to_vec());
    add("oversized-tag", format!("title: !<{}> x\n", "t".repeat(4096)).into_bytes());
    add("oversized-tag-handle", format!("%TAG !e! tag:{},2026:\n---\ntitle: !e!{} x\n", "z".repeat(2048), "y".repeat(2048)).into_bytes());
    add("many-directives", { let mut s = String::new(); for i in 0..200 { s.push_str(&format!("%TAG !t{i}! tag:x{i}:\n")); } s.push_str("---\ntitle: a\n"); s.into_bytes() });
    add("long-anchor", format!("title: &{} a\nid: *{}\n", "n".repeat(3000), "n".repeat(3000)).into_bytes());
    add("plain-scalar-40k", format!("title: {}\n", "a".repeat(40 * 1024)).into_bytes());
    add("dq-scalar-40k-escapes", format!("title: \"{}\"\n", "\\x41\\u00e9".repeat(5 * 1024)).into_bytes());
    add("sq-scalar-40k", format!("title: '{}'\n", "b''".repeat(14 * 1024)).into_bytes());
    add("block-literal-40k-lines", { let mut s = String::from("title: |\n"); for _ in 0..4000 { s.push_str("  0123456789\n"); } s.into_bytes() });
    add("folded-40k", { let mut s = String::from("title: >-\n"); for _ in 0..4000 { s.push_str("  word word\n\n"); } s.into_bytes() });
    add("indent-indicator-bad", b"title: |99\n  a\n".to_vec());
    add("explicit-key-deep", format!("{}x\n", "? ".repeat(200)).into_bytes());
    add("complex-key", b"? [a, b, {c: d}]\n: v\ntitle: t\n".to_vec());
    add("multi-doc-with-alias", b"--- &a\ntitle: x\n---\nid: *a\n...\n---\n".to_vec());
    add("many-keys-4k", { let mut s = String::new(); for i in 0..4096 { s.push_str(&format!("k{i}: {i}\n")); } s.into_bytes() });
    add("duplicate-keys", b"title: a\ntitle: b\ntitle: c\n".to_vec());
    add("float-and-int-edges", b"quality: {score: .nan, uses: 99999999999999999999}\ntimeout: -1\nscore: 1e999\n".to_vec());
    add("mixed-crlf-tabs", b"title:\tx\r\nid:\r\n\t- a\r\n".to_vec());
    add("flow-in-block-broken", b"keywords: [a, b\nid: c]\n".to_vec());
    add("unclosed-flow-eof", b"keywords: [a, b, {c: d".to_vec());
    add("empty", b"".to_vec());
    add("only-comment", b"# c\n".to_vec());
    add("whitespace-and-bom-only", b"\xef\xbb\xbf   \n\t\n".to_vec());
    v
}

fn describe<T: std::fmt::Debug>(r: std::thread::Result<Result<T, serde_yaml::Error>>) -> String {
    match r {
        Ok(Ok(_)) => "ok".to_string(),
        Ok(Err(e)) => format!("err: {}", e.to_string().lines().next().unwrap_or("").chars().take(90).collect::<String>()),
        Err(_) => "PANIC (safe, caught)".to_string(),
    }
}

fn main() {
    let cases = corpus();
    println!("EXP-202: {} inputs, serde_yaml 0.9.34 / unsafe-libyaml 0.2.11", cases.len());
    for (name, bytes) in &cases {
        let b = bytes.clone();
        let r_value = describe(std::panic::catch_unwind(|| serde_yaml::from_slice::<serde_yaml::Value>(&b)));
        let b = bytes.clone();
        let r_cmd = describe(std::panic::catch_unwind(|| serde_yaml::from_slice::<CommandDefinition>(&b)));
        let b = bytes.clone();
        let r_inj = describe(std::panic::catch_unwind(|| serde_yaml::from_slice::<InjectorFrontmatter>(&b)));
        let b = bytes.clone();
        let r_learn = describe(std::panic::catch_unwind(|| serde_yaml::from_slice::<LearningFrontmatter>(&b)));
        // from_str where the bytes happen to be UTF-8 (the workspace call shape).
        let r_str = match std::str::from_utf8(bytes) {
            Ok(s) => describe(std::panic::catch_unwind(|| serde_yaml::from_str::<serde_yaml::Value>(s))),
            Err(_) => "n/a (not UTF-8; workspace from_str unreachable, from_slice covers it)".to_string(),
        };
        println!("[{name}] len={} value={r_value} | cmd={r_cmd} | injector={r_inj} | learning={r_learn} | from_str={r_str}", bytes.len());
    }
    println!("EXP-202 DONE: inspect the Miri log for any dependency-frame UB report");
}
