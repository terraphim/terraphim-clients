//! Build the committed memory benchmark fixture from a learnings directory.
//!
//! This is step 1 of the judge-free memory measurement plan
//! (terraphim/terraphim-clients#255, issue #259). It is invoked by
//! `scripts/build_memory_fixture.sh` and writes:
//!
//! * `corpus.jsonl`: at most [`MAX_ITEMS`] `MemoryItem` records, one per line.
//! * `queries.jsonl`: between [`MIN_QUERIES`] and [`MAX_QUERIES`] records of
//!   the shape `{"query": "...", "expected_ids": ["..."]}`.
//!
//! Selection and ground truth are mechanical, so no relevance judgement is
//! invented by hand:
//!
//! * every `correction-*.md` file becomes one item, and its `## Original`
//!   text becomes a query whose expected id is that correction;
//! * every `learning-*.md` file is grouped by its redacted, whitespace
//!   normalised command; a command captured more than once is a
//!   "repeated failure cluster". The earliest capture of the cluster becomes
//!   the corpus item and the command becomes a query whose expected id is
//!   that earliest capture. `access_count` records the cluster size.
//!
//! Every text field is passed through the capture module's
//! [`redact_secrets`] and through a structural pass that removes user@host
//! pairs, IPv4 addresses, ssh targets, fully qualified host names, home
//! directories, 1Password references, bearer tokens, credential-shaped
//! values and long hexadecimal runs. The rules are structural on purpose:
//! this file is committed to a repository that is mirrored publicly, so it
//! must not carry a list of the very names it redacts.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use chrono::{DateTime, Utc};
use regex::Regex;
use sha2::{Digest, Sha256};
use terraphim_agent::learnings::{CapturedLearning, CorrectionEvent, redact_secrets};
use terraphim_agent_evolution::{ImportanceLevel, MemoryItem, MemoryItemType};

/// Upper bound on corpus records (acceptance bullet 1 of #255).
const MAX_ITEMS: usize = 200;
/// Lower bound on query records.
const MIN_QUERIES: usize = 20;
/// Upper bound on query records.
const MAX_QUERIES: usize = 50;
/// Error output is capped so every corpus line stays reviewable.
const ERROR_OUTPUT_CAP_CHARS: usize = 2000;

/// Public developer domains that carry no private information and are kept.
const PUBLIC_HOST_ALLOWLIST: &[&str] = &[
    "github.com",
    "githubusercontent.com",
    "crates.io",
    "docs.rs",
    "rust-lang.org",
    "rustup.rs",
    "npmjs.com",
    "npmjs.org",
    "pypi.org",
    "python.org",
    "docker.io",
    "docker.com",
    "ghcr.io",
    "cloudflare.com",
    "example.com",
    "localhost",
];

/// Top-level domains treated as host names when they end a dotted label run.
/// Source-file extensions (`rs`, `sh`, `go`, `py`, `md`, ...) are deliberately
/// absent so file names are not mistaken for hosts.
const HOST_TLDS: &[&str] = &[
    "cloud",
    "ai",
    "com",
    "io",
    "net",
    "org",
    "dev",
    "engineer",
    "local",
    "lan",
    "internal",
    "localhost",
];

struct Redactor {
    user_at_host: Regex,
    ipv4: Regex,
    ssh_target: Regex,
    dotted: Regex,
    op_ref_quoted: Regex,
    op_ref: Regex,
    bearer: Regex,
    credential: Regex,
    long_hex: Regex,
    macos_home: Regex,
    linux_home: Regex,
    org_client_dir: Regex,
    ansi_escape: Regex,
    host_label: Regex,
    syslog_host: Regex,
    url_credentials: Regex,
}

impl Redactor {
    fn new() -> Self {
        Self {
            user_at_host: Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9][A-Za-z0-9.-]*").unwrap(),
            ipv4: Regex::new(r"\b(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})\b").unwrap(),
            ssh_target: Regex::new(
                r#"\b(ssh|scp)(\s+(?:-o\s+\S+\s+|-[A-Za-z]\s+\S+\s+|-[A-Za-z]+\s+)*)([A-Za-z][A-Za-z0-9_.-]*)(\s|["':]|$)"#,
            )
            .unwrap(),
            dotted: Regex::new(r"\b[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+\b").unwrap(),
            op_ref_quoted: Regex::new(r#"(["'])op://[^"'\n]+(["'])"#).unwrap(),
            op_ref: Regex::new(r"op://\S+").unwrap(),
            bearer: Regex::new(r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]+").unwrap(),
            credential: Regex::new(
                r#"(?i)(token|secret|password|passwd|api[_-]?key)(\s*[=:]\s*|\s+)(['"]?)[A-Za-z0-9/+_.~-]{8,}"#,
            )
            .unwrap(),
            long_hex: Regex::new(r"\b[0-9a-fA-F]{32,}\b").unwrap(),
            macos_home: Regex::new(r"/Users/[A-Za-z0-9._-]+").unwrap(),
            linux_home: Regex::new(r"/home/[A-Za-z0-9._-]+").unwrap(),
            org_client_dir: Regex::new(r"zestic-ai/[A-Za-z0-9._-]+").unwrap(),
            ansi_escape: Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").unwrap(),
            host_label: Regex::new(r"(?i)\b(worker|host|hostname)(\s*[:=]\s*)[A-Za-z0-9][A-Za-z0-9._-]*")
                .unwrap(),
            // "Apr 22 21:22:16 <host> proc[pid]:" syslog and journalctl lines.
            syslog_host: Regex::new(r"(?m)^([A-Z][a-z]{2} {1,2}\d{1,2} \d{2}:\d{2}:\d{2}) \S+ ")
                .unwrap(),
            // scheme://user:password@host, host included so the pair is canonical
            url_credentials: Regex::new(r"://[^/\s:@]+:[^/\s@]+@[A-Za-z0-9.-]+").unwrap(),
        }
    }

    fn redact(&self, text: &str) -> String {
        let mut s = self.ansi_escape.replace_all(text, "").to_string();
        s = self
            .url_credentials
            .replace_all(&s, "://[USER]@[HOST]")
            .to_string();
        s = self.syslog_host.replace_all(&s, "${1} [HOST] ").to_string();
        s = self
            .user_at_host
            .replace_all(&s, "[USER]@[HOST]")
            .to_string();
        s = self
            .ipv4
            .replace_all(&s, |c: &regex::Captures| {
                let whole = &c[0];
                if whole == "127.0.0.1" || whole == "0.0.0.0" {
                    whole.to_string()
                } else {
                    "[IP]".to_string()
                }
            })
            .to_string();
        s = self
            .ssh_target
            .replace_all(&s, |c: &regex::Captures| {
                format!("{}{}[HOST]{}", &c[1], &c[2], &c[4])
            })
            .to_string();
        s = self
            .dotted
            .replace_all(&s, |c: &regex::Captures| {
                let whole = &c[0];
                if is_private_host(whole) {
                    "[HOST]".to_string()
                } else {
                    whole.to_string()
                }
            })
            .to_string();
        s = self
            .op_ref_quoted
            .replace_all(&s, "${1}op://[REDACTED]${2}")
            .to_string();
        s = self.op_ref.replace_all(&s, "op://[REDACTED]").to_string();
        s = self
            .host_label
            .replace_all(&s, "${1}${2}[HOST]")
            .to_string();
        s = self.bearer.replace_all(&s, "Bearer [REDACTED]").to_string();
        s = self
            .credential
            .replace_all(&s, "${1}${2}${3}[REDACTED]")
            .to_string();
        s = self.long_hex.replace_all(&s, "[HEX_REDACTED]").to_string();
        s = self.macos_home.replace_all(&s, "/Users/[USER]").to_string();
        s = self.linux_home.replace_all(&s, "/home/[USER]").to_string();
        s = self
            .org_client_dir
            .replace_all(&s, "zestic-ai/[CLIENT]")
            .to_string();
        redact_secrets(&s)
    }
}

/// A dotted label run is a private host name when its last label is a known
/// TLD, it is not a bare version number, and its apex is not allowlisted.
fn is_private_host(candidate: &str) -> bool {
    let lower = candidate.to_ascii_lowercase();
    let labels: Vec<&str> = lower.split('.').collect();
    let Some(tld) = labels.last() else {
        return false;
    };
    if !HOST_TLDS.contains(tld) {
        return false;
    }
    if labels.iter().all(|l| l.chars().all(|c| c.is_ascii_digit())) {
        return false;
    }
    let apex = if labels.len() >= 2 {
        format!("{}.{}", labels[labels.len() - 2], labels[labels.len() - 1])
    } else {
        lower.clone()
    };
    !(PUBLIC_HOST_ALLOWLIST.contains(&apex.as_str()) || PUBLIC_HOST_ALLOWLIST.contains(tld))
}

fn normalise_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The front matter parser keeps only the first line of a multi-line
/// `command:` value, so the full command is read from the `## Command`
/// section of the body instead.
fn full_command_from_body(markdown: &str) -> Option<String> {
    let idx = markdown.find("## Command\n")?;
    let after = &markdown[idx + "## Command\n".len()..];
    let start = after.find('`')? + 1;
    let rest = &after[start..];
    let end = rest.find("`\n")?;
    Some(rest[..end].to_string())
}

fn truncate_chars(text: &str, cap: usize) -> String {
    if text.chars().count() <= cap {
        return text.to_string();
    }
    let mut out: String = text.chars().take(cap).collect();
    out.push_str("\n[truncated]");
    out
}

struct Learning {
    id: String,
    captured_at: DateTime<Utc>,
    exit_code: i32,
    tags: Vec<String>,
    command: String,
    error_output: String,
}

struct Cluster {
    key: String,
    size: usize,
    representative: Learning,
}

#[derive(serde::Serialize)]
struct Query {
    query: String,
    expected_ids: Vec<String>,
}

fn read_dir_sorted(dir: &Path, prefix: &str) -> Result<Vec<PathBuf>, String> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(prefix) && n.ends_with(".md"))
                .unwrap_or(false)
        })
        .collect();
    paths.sort();
    Ok(paths)
}

/// Ids are `<uuid-hex>-<unix-millis>`; the suffix is the capture time. The
/// front matter parser falls back to the current time when a multi-line
/// command contains `---`, so the id is the deterministic source of
/// `created_at`.
fn created_at_from_id(id: &str) -> Option<DateTime<Utc>> {
    let (_, ts) = id.split_once('-')?;
    DateTime::<Utc>::from_timestamp_millis(ts.parse().ok()?)
}

fn id_is_well_formed(id: &str) -> bool {
    let Some((uuid, ts)) = id.split_once('-') else {
        return false;
    };
    uuid.len() == 32
        && uuid.chars().all(|c| c.is_ascii_hexdigit())
        && !ts.is_empty()
        && ts.chars().all(|c| c.is_ascii_digit())
}

fn load_learnings(dir: &Path, redactor: &Redactor) -> Result<Vec<Learning>, String> {
    let mut out = Vec::new();
    let mut skipped_unparsed = 0usize;
    let mut skipped_client = 0usize;
    for path in read_dir_sorted(dir, "learning-")? {
        let text = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let Some(parsed) = CapturedLearning::from_markdown(&text) else {
            skipped_unparsed += 1;
            continue;
        };
        if !id_is_well_formed(&parsed.id) {
            skipped_unparsed += 1;
            continue;
        }
        let raw_command = full_command_from_body(&text).unwrap_or_else(|| parsed.command.clone());
        // Client work is excluded by path prefix, not by client name: a capture
        // whose working directory or command refers to the client tree is left
        // out rather than redacted.
        if parsed.context.working_dir.contains("/zestic-ai/")
            || raw_command.contains("zestic-ai/")
            || parsed.error_output.contains("zestic-ai/")
        {
            skipped_client += 1;
            continue;
        }
        let Some(captured_at) = created_at_from_id(&parsed.id) else {
            skipped_unparsed += 1;
            continue;
        };
        let command = normalise_whitespace(&redactor.redact(&raw_command));
        if command.is_empty() {
            skipped_unparsed += 1;
            continue;
        }
        out.push(Learning {
            id: parsed.id,
            captured_at,
            exit_code: parsed.exit_code,
            tags: parsed.tags,
            command,
            error_output: truncate_chars(
                &redactor.redact(&parsed.error_output),
                ERROR_OUTPUT_CAP_CHARS,
            ),
        });
    }
    eprintln!(
        "learnings: {} loaded, {} skipped (unparsed or empty), {} skipped (client path)",
        out.len(),
        skipped_unparsed,
        skipped_client
    );
    Ok(out)
}

fn load_corrections(dir: &Path) -> Result<Vec<CorrectionEvent>, String> {
    let mut out = Vec::new();
    for path in read_dir_sorted(dir, "correction-")? {
        let text = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        match CorrectionEvent::from_markdown(&text) {
            Some(c) if id_is_well_formed(&c.id) && !c.original.trim().is_empty() => out.push(c),
            _ => eprintln!("correction skipped (unparsed or empty): {}", path.display()),
        }
    }
    out.sort_by(|a, b| {
        a.context
            .captured_at
            .cmp(&b.context.captured_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(out)
}

fn cluster_repeated(learnings: Vec<Learning>) -> Vec<Cluster> {
    let mut groups: HashMap<String, Vec<Learning>> = HashMap::new();
    for l in learnings {
        groups.entry(l.command.clone()).or_default().push(l);
    }
    let mut clusters: Vec<Cluster> = groups
        .into_iter()
        .filter(|(_, members)| members.len() >= 2)
        .map(|(key, mut members)| {
            members.sort_by(|a, b| {
                a.captured_at
                    .cmp(&b.captured_at)
                    .then_with(|| a.id.cmp(&b.id))
            });
            let size = members.len();
            let representative = members.into_iter().next().expect("non-empty cluster");
            Cluster {
                key,
                size,
                representative,
            }
        })
        .collect();
    clusters.sort_by(|a, b| {
        b.size
            .cmp(&a.size)
            .then_with(|| {
                a.representative
                    .captured_at
                    .cmp(&b.representative.captured_at)
            })
            .then_with(|| a.representative.id.cmp(&b.representative.id))
    });
    clusters
}

fn learning_item(cluster: &Cluster) -> MemoryItem {
    let l = &cluster.representative;
    let content = format!(
        "Command: {}\nExit code: {}\nError output:\n{}",
        l.command, l.exit_code, l.error_output
    );
    let mut associations = HashMap::new();
    associations.insert("origin".to_string(), "learning".to_string());
    MemoryItem {
        id: l.id.clone(),
        item_type: MemoryItemType::Experience,
        content,
        created_at: l.captured_at,
        last_accessed: None,
        access_count: u32::try_from(cluster.size).unwrap_or(u32::MAX),
        importance: ImportanceLevel::Medium,
        tags: l.tags.clone(),
        associations,
    }
}

fn correction_item(c: &CorrectionEvent, redactor: &Redactor) -> (MemoryItem, String) {
    let created_at = created_at_from_id(&c.id).unwrap_or(c.context.captured_at);
    let original = normalise_whitespace(&redactor.redact(&c.original));
    let corrected = normalise_whitespace(&redactor.redact(&c.corrected));
    let context = normalise_whitespace(&redactor.redact(&c.context_description));
    let mut content = format!(
        "Correction ({}): {}\nCorrected: {}",
        c.correction_type, original, corrected
    );
    if !context.is_empty() {
        content.push_str("\nContext: ");
        content.push_str(&context);
    }
    let mut associations = HashMap::new();
    associations.insert("origin".to_string(), "correction".to_string());
    let item = MemoryItem {
        id: c.id.clone(),
        item_type: MemoryItemType::LessonLearned,
        content,
        created_at,
        last_accessed: None,
        access_count: 0,
        importance: ImportanceLevel::Medium,
        tags: vec![
            "correction".to_string(),
            format!("type:{}", c.correction_type),
        ],
        associations,
    };
    (item, original)
}

/// Serialise with a stable key order for `associations` so the corpus bytes,
/// and therefore its SHA-256, reproduce run to run.
fn item_to_json_line(item: &MemoryItem) -> Result<String, String> {
    let mut value = serde_json::to_value(item).map_err(|e| e.to_string())?;
    let sorted: BTreeMap<&String, &String> = item.associations.iter().collect();
    let mut map = serde_json::Map::new();
    for (k, v) in sorted {
        map.insert(k.clone(), serde_json::Value::String(v.clone()));
    }
    value["associations"] = serde_json::Value::Object(map);
    serde_json::to_string(&value).map_err(|e| e.to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn run(learnings_dir: &Path, out_dir: &Path) -> Result<(), String> {
    let redactor = Redactor::new();
    let corrections = load_corrections(learnings_dir)?;
    let learnings = load_learnings(learnings_dir, &redactor)?;
    let clusters = cluster_repeated(learnings);
    eprintln!(
        "corrections: {}, repeated-failure clusters: {}",
        corrections.len(),
        clusters.len()
    );

    let mut items: Vec<MemoryItem> = Vec::new();
    let mut queries: Vec<Query> = Vec::new();

    for c in &corrections {
        let (item, original) = correction_item(c, &redactor);
        queries.push(Query {
            query: original,
            expected_ids: vec![item.id.clone()],
        });
        items.push(item);
    }

    let cluster_budget = MAX_ITEMS.saturating_sub(items.len());
    for cluster in clusters.iter().take(cluster_budget) {
        let item = learning_item(cluster);
        if queries.len() < MAX_QUERIES {
            queries.push(Query {
                query: cluster.key.clone(),
                expected_ids: vec![item.id.clone()],
            });
        }
        items.push(item);
    }

    if queries.len() < MIN_QUERIES {
        return Err(format!(
            "only {} queries could be derived; at least {} are required",
            queries.len(),
            MIN_QUERIES
        ));
    }
    {
        let mut seen = std::collections::HashSet::new();
        for q in &queries {
            if !seen.insert(q.query.as_str()) {
                return Err(format!("duplicate query text: {}", q.query));
            }
        }
    }

    items.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    queries.sort_by(|a, b| a.expected_ids.cmp(&b.expected_ids));

    let mut corpus = String::new();
    for item in &items {
        corpus.push_str(&item_to_json_line(item)?);
        corpus.push('\n');
    }
    let mut queries_text = String::new();
    for q in &queries {
        queries_text.push_str(&serde_json::to_string(q).map_err(|e| e.to_string())?);
        queries_text.push('\n');
    }

    fs::create_dir_all(out_dir).map_err(|e| format!("cannot create {}: {e}", out_dir.display()))?;
    let corpus_path = out_dir.join("corpus.jsonl");
    let queries_path = out_dir.join("queries.jsonl");
    fs::write(&corpus_path, &corpus).map_err(|e| format!("cannot write corpus: {e}"))?;
    fs::write(&queries_path, &queries_text).map_err(|e| format!("cannot write queries: {e}"))?;

    eprintln!("cluster table (size, representative id, command prefix):");
    for cluster in clusters.iter().take(cluster_budget) {
        let prefix: String = cluster.key.chars().take(72).collect();
        eprintln!(
            "  {:>3}  {}  {}",
            cluster.size, cluster.representative.id, prefix
        );
    }
    println!("corpus_items={}", items.len());
    println!("queries={}", queries.len());
    println!("corpus_sha256={}", sha256_hex(corpus.as_bytes()));
    println!("queries_sha256={}", sha256_hex(queries_text.as_bytes()));
    println!("corpus_path={}", corpus_path.display());
    println!("queries_path={}", queries_path.display());
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: build_memory_fixture <learnings_dir> <out_dir>");
        return ExitCode::from(2);
    }
    match run(Path::new(&args[1]), Path::new(&args[2])) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
