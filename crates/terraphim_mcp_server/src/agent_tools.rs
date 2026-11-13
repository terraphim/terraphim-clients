//! Typed local-agent MCP contracts. Storage is supplied explicitly by the host.
use rmcp::model::{CallToolResult, Content, ErrorData, Tool, ToolAnnotations};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};
use terraphim_agent::agent_store::{AgentStore, LearningRecord, SavedLearning};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CaptureMemory {
    /// Authored UTF-8 content, at most 1,000,000 bytes. Never replaced by a tag.
    #[schemars(length(min = 1, max = 1_000_000))]
    content: String,
    /// Optional source label, at most 512 UTF-8 bytes.
    #[schemars(length(min = 1, max = 512))]
    provenance_tag: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Id {
    /// Exact returned memory ID; prefixes and filesystem paths are refused.
    #[schemars(length(min = 1, max = 200))]
    id: String,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Page {
    #[serde(default = "default_limit")]
    #[schemars(range(min = 1, max = 100))]
    limit: usize,
    #[serde(default)]
    offset: usize,
}
fn default_limit() -> usize {
    20
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CaptureLearning {
    /// Observed command, not a command for the server to execute. At most 65,536 bytes.
    #[schemars(length(min = 1, max = 65_536))]
    command: String,
    /// Actual observed stderr/error text, at most 524,288 UTF-8 bytes.
    #[schemars(length(min = 1, max = 524_288))]
    error_output: String,
    /// Observed non-zero exit code. Capture never runs the command to obtain it.
    exit_code: i32,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct QueryLearning {
    /// Case-insensitive text query over stored failures and explicit corrections.
    #[schemars(length(min = 1, max = 4_096))]
    query: String,
    #[serde(default = "default_limit")]
    #[schemars(range(min = 1, max = 100))]
    limit: usize,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CorrectLearning {
    #[schemars(length(min = 1, max = 200))]
    id: String,
    /// Annotation only; this does not create a ToolPreference rule or activate a hook.
    #[schemars(length(min = 1, max = 65_536))]
    correction: String,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AddCorrection {
    #[schemars(length(min = 1, max = 65_536))]
    original: String,
    #[schemars(length(min = 1, max = 65_536))]
    corrected: String,
    #[serde(default)]
    #[schemars(length(max = 65_536))]
    context: String,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Serialize, JsonSchema)]
struct Scope {
    /// Canonical explicit --agent-data-dir, shared with the CLI environment override.
    root: String,
    memory_file: String,
    learning_directory: String,
}
#[derive(Serialize, JsonSchema)]
struct Envelope<T> {
    storage: Scope,
    data: T,
}
#[derive(Serialize, Deserialize, JsonSchema)]
struct MemoryView {
    id: String,
    content: String,
    tags: Vec<String>,
    created_at: String,
    item_type: String,
    importance: String,
    last_accessed: Option<String>,
    access_count: u32,
    associations: HashMap<String, String>,
}
#[derive(Serialize, JsonSchema)]
struct MemoryPage {
    items: Vec<MemoryView>,
    count: usize,
}
#[derive(Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LearningView {
    Failure {
        id: String,
        path: String,
        command: String,
        error_output: String,
        exit_code: i32,
        correction: Option<String>,
    },
    Correction {
        id: String,
        path: String,
        original: String,
        corrected: String,
        context: String,
    },
}
impl From<SavedLearning> for LearningView {
    fn from(saved: SavedLearning) -> Self {
        let path = saved.path.to_string_lossy().into_owned();
        match saved.record {
            LearningRecord::Failure(record) => Self::Failure {
                id: record.id,
                path,
                command: record.command,
                error_output: record.error_output,
                exit_code: record.exit_code,
                correction: record.correction,
            },
            LearningRecord::Correction(record) => Self::Correction {
                id: record.id,
                path,
                original: record.original,
                corrected: record.corrected,
                context: record.context_description,
            },
        }
    }
}
#[derive(Serialize, JsonSchema)]
struct LearningPage {
    items: Vec<LearningView>,
    count: usize,
}
#[derive(Serialize, JsonSchema)]
struct Mapping {
    pattern: String,
    concept_id: u64,
    replacement: String,
    display: String,
}
#[derive(Serialize, JsonSchema)]
struct Compilation {
    path: String,
    count: usize,
    mappings: Vec<Mapping>,
    activated: bool,
}
#[derive(Serialize, JsonSchema)]
struct Note {
    path: String,
    markdown: String,
}
#[derive(Serialize, JsonSchema)]
struct Export {
    directory: String,
    files: Vec<Note>,
    registered: bool,
}

fn tool<I: JsonSchema, O: JsonSchema>(
    name: &'static str,
    description: &'static str,
    read_only: bool,
    destructive: bool,
) -> Tool {
    Tool {
        name: name.into(),
        title: None,
        description: Some(description.into()),
        input_schema: Arc::new(
            schema_for!(I)
                .as_object()
                .expect("object input schema")
                .clone(),
        ),
        output_schema: Some(Arc::new(
            schema_for!(Envelope<O>)
                .as_object()
                .expect("object output schema")
                .clone(),
        )),
        annotations: Some(ToolAnnotations {
            read_only_hint: Some(read_only),
            destructive_hint: Some(destructive),
            idempotent_hint: Some(read_only),
            open_world_hint: Some(false),
            ..Default::default()
        }),
        icons: None,
        meta: None,
    }
}
pub(crate) fn tools() -> Vec<Tool> {
    vec![
        tool::<CaptureMemory, MemoryView>(
            "agent_memory_capture",
            "Save authored content and optional provenance tag to the explicit local store, then return actual readback. Existing memory semantics retain the newest 100 short-term items. No role/project isolation is inferred from cwd.",
            false,
            true,
        ),
        tool::<Id, MemoryView>(
            "agent_memory_show",
            "Read one exact saved memory ID from the configured local store. Unknown IDs return an error.",
            true,
            false,
        ),
        tool::<Page, MemoryPage>(
            "agent_memory_list",
            "List actual saved memory records with stable bounded pagination. This is local store enumeration, not semantic retrieval.",
            true,
            false,
        ),
        tool::<CaptureLearning, LearningView>(
            "agent_learning_capture",
            "Record a supplied failed-command observation with secret redaction. Does not execute the command or activate a hook.",
            false,
            false,
        ),
        tool::<QueryLearning, LearningPage>(
            "agent_learning_query",
            "Query actual stored failures and explicit corrections by case-insensitive text. This does not execute commands or rank by a role graph.",
            true,
            false,
        ),
        tool::<CorrectLearning, LearningView>(
            "agent_learning_correct",
            "Annotate one exact failed-learning ID with a correction. Annotation alone creates no compiled rule.",
            false,
            true,
        ),
        tool::<AddCorrection, LearningView>(
            "agent_correction_add",
            "Create an explicit ToolPreference correction using existing Terraphim types. Compile and KG export are separate calls; no hook activation.",
            false,
            false,
        ),
        tool::<Empty, Compilation>(
            "agent_correction_compile",
            "Compile existing ToolPreference corrections into a fresh thesaurus JSON under the configured root. Return actual mappings and saved path. No active file is replaced and no hook is activated.",
            false,
            false,
        ),
        tool::<Empty, Export>(
            "agent_correction_export_kg",
            "Export ToolPreference corrections into fresh Logseq-compatible Markdown notes containing synonyms::. Return actual saved notes. This does not register a graph in a role or activate a hook.",
            false,
            false,
        ),
    ]
}
fn parse<T: DeserializeOwned>(arguments: Value) -> Result<T, ErrorData> {
    serde_json::from_value(arguments).map_err(|error| {
        ErrorData::invalid_params(format!("Invalid agent tool arguments: {error}"), None)
    })
}
fn response<T: Serialize>(store: &AgentStore, data: T) -> anyhow::Result<CallToolResult> {
    Ok(CallToolResult::structured(serde_json::to_value(
        Envelope {
            storage: Scope {
                root: store.root().to_string_lossy().into_owned(),
                memory_file: store.memory_path().to_string_lossy().into_owned(),
                learning_directory: store
                    .root()
                    .join("terraphim/learnings")
                    .to_string_lossy()
                    .into_owned(),
            },
            data,
        },
    )?))
}
fn memory_view(value: impl Serialize) -> anyhow::Result<MemoryView> {
    Ok(serde_json::from_value(serde_json::to_value(value)?)?)
}

/// Cancellation is honoured before acquiring the real store transaction. Once
/// a bounded synchronous commit starts, cancellation is not a rollback.
pub(crate) async fn call_guarded(
    store: &tokio::sync::Mutex<AgentStore>,
    cancellation: &tokio_util::sync::CancellationToken,
    name: &str,
    arguments: Value,
) -> Result<CallToolResult, ErrorData> {
    let guard = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err(ErrorData::internal_error("Request cancelled before storage operation", None)),
        guard = store.lock() => guard,
    };
    if cancellation.is_cancelled() {
        return Err(ErrorData::internal_error(
            "Request cancelled before storage operation",
            None,
        ));
    }
    call(&guard, name, arguments)
}

pub(crate) fn call(
    store: &AgentStore,
    name: &str,
    arguments: Value,
) -> Result<CallToolResult, ErrorData> {
    // Parse first: serde rejects wrong types, integer overflow and unknown fields.
    // Application failures retain isError=true and never emit success-shaped data.
    let result: anyhow::Result<CallToolResult> = match name {
        "agent_memory_capture" => {
            let args: CaptureMemory = parse(arguments)?;
            (|| {
                response(
                    store,
                    memory_view(
                        store.memory_capture(&args.content, args.provenance_tag.as_deref())?,
                    )?,
                )
            })()
        }
        "agent_memory_show" => {
            let args: Id = parse(arguments)?;
            (|| response(store, memory_view(store.memory_show(&args.id)?)?))()
        }
        "agent_memory_list" => {
            let args: Page = parse(arguments)?;
            (|| {
                let items = store
                    .memory_list(args.limit, args.offset)?
                    .into_iter()
                    .map(memory_view)
                    .collect::<anyhow::Result<Vec<_>>>()?;
                let count = items.len();
                response(store, MemoryPage { items, count })
            })()
        }
        "agent_learning_capture" => {
            let args: CaptureLearning = parse(arguments)?;
            (|| {
                response(
                    store,
                    LearningView::from(store.learning_capture(
                        &args.command,
                        &args.error_output,
                        args.exit_code,
                    )?),
                )
            })()
        }
        "agent_learning_query" => {
            let args: QueryLearning = parse(arguments)?;
            (|| {
                let items: Vec<_> = store
                    .learning_query(&args.query, args.limit)?
                    .into_iter()
                    .map(LearningView::from)
                    .collect();
                let count = items.len();
                response(store, LearningPage { items, count })
            })()
        }
        "agent_learning_correct" => {
            let args: CorrectLearning = parse(arguments)?;
            (|| {
                response(
                    store,
                    LearningView::from(store.learning_correct(&args.id, &args.correction)?),
                )
            })()
        }
        "agent_correction_add" => {
            let args: AddCorrection = parse(arguments)?;
            (|| {
                response(
                    store,
                    LearningView::from(store.correction_add(
                        &args.original,
                        &args.corrected,
                        &args.context,
                    )?),
                )
            })()
        }
        "agent_correction_compile" => {
            let _: Empty = parse(arguments)?;
            (|| {
                let compiled = store.correction_compile()?;
                let mut mappings: Vec<_> = (&compiled.thesaurus)
                    .into_iter()
                    .map(|(pattern, term)| Mapping {
                        pattern: pattern.to_string(),
                        concept_id: term.id,
                        replacement: term.value.to_string(),
                        display: term.display().to_owned(),
                    })
                    .collect();
                mappings.sort_by(|a, b| a.pattern.cmp(&b.pattern));
                response(
                    store,
                    Compilation {
                        path: compiled.path.to_string_lossy().into_owned(),
                        count: compiled.mappings,
                        mappings,
                        activated: false,
                    },
                )
            })()
        }
        "agent_correction_export_kg" => {
            let _: Empty = parse(arguments)?;
            (|| {
                let exported = store.correction_export_kg()?;
                response(
                    store,
                    Export {
                        directory: exported.directory.to_string_lossy().into_owned(),
                        files: exported
                            .files
                            .into_iter()
                            .map(|note| Note {
                                path: note.path.to_string_lossy().into_owned(),
                                markdown: note.markdown,
                            })
                            .collect(),
                        registered: false,
                    },
                )
            })()
        }
        _ => return Err(ErrorData::invalid_params("Unknown agent tool", None)),
    };
    Ok(match result {
        Ok(result) => result,
        Err(error) => CallToolResult::error(vec![Content::text(
            json!({"error":format!("{error:#}"),"storage_root":store.root()}).to_string(),
        )]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_while_queued_never_writes_but_cancellation_after_commit_is_not_rollback()
    {
        let root = tempfile::tempdir().unwrap();
        let store = tokio::sync::Mutex::new(AgentStore::open(root.path()).unwrap());
        let held = store.lock().await;
        let cancellation = tokio_util::sync::CancellationToken::new();
        let operation = call_guarded(
            &store,
            &cancellation,
            "agent_memory_capture",
            json!({"content":"cancelled pending input"}),
        );
        tokio::pin!(operation);
        // The real mutex is held: polling the actual operation cannot mutate storage.
        tokio::select! {
            biased;
            result = &mut operation => panic!("queued operation unexpectedly finished: {result:?}"),
            _ = tokio::task::yield_now() => (),
        }
        cancellation.cancel();
        assert!(operation.await.is_err());
        assert!(!held.memory_path().exists());
        drop(held);
        let committed_token = tokio_util::sync::CancellationToken::new();
        let committed = call_guarded(
            &store,
            &committed_token,
            "agent_memory_capture",
            json!({"content":"acknowledged persisted output"}),
        )
        .await
        .unwrap();
        committed_token.cancel();
        let id = committed.structured_content.as_ref().unwrap()["data"]["id"]
            .as_str()
            .unwrap();
        let persisted = store.lock().await.memory_show(id).unwrap();
        assert_eq!(persisted.content, "acknowledged persisted output");
    }
}
