//! Real stdio, CLI, filesystem and failed-command evidence for local agent tools.
use anyhow::{Context, Result, ensure};
use rmcp::{
    RoleClient,
    model::CallToolRequestParam,
    service::{RunningService, ServiceExt},
    transport::TokioChildProcess,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::process::Command;

type Client = RunningService<RoleClient, ()>;
struct Fixture {
    root: tempfile::TempDir,
    project: PathBuf,
    settings: PathBuf,
    store: PathBuf,
}
impl Fixture {
    fn new() -> Result<Self> {
        let root = tempfile::tempdir()?;
        let project = root.path().join("public project");
        let settings = root.path().join("settings");
        let store = root.path().join("explicit agent store");
        fs::create_dir_all(project.join(".terraphim"))?;
        fs::create_dir_all(&settings)?;
        let mut roles = serde_json::Map::new();
        for name in ["Default", "Engineer", "System Operator"] {
            roles.insert(name.into(), json!({"name":name,"relevance_function":"title-scorer","terraphim_it":false,"theme":"lumen","kg":null,"haystacks":[],"extra":{}}));
        }
        fs::write(
            project.join(".terraphim/config.json"),
            serde_json::to_vec_pretty(
                &json!({"selected_role":"Default","default_role":"Default","roles":roles}),
            )?,
        )?;
        fs::write(
            settings.join("settings.toml"),
            format!(
                "server_hostname = \"127.0.0.1:8000\"\napi_endpoint = \"http://localhost:8000/api\"\ninitialized = true\ndefault_data_path = {}\n[profiles.memory]\ntype = \"memory\"\n",
                serde_json::to_string(&root.path().join("data"))?
            ),
        )?;
        Ok(Self {
            root,
            project,
            settings,
            store,
        })
    }
    fn command(&self, binary: &Path) -> Result<Command> {
        let mut command = Command::new(binary);
        command
            .current_dir(&self.project)
            .kill_on_drop(true)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").context("PATH")?)
            .env("TERRAPHIM_SETTINGS_PATH", &self.settings)
            .env("TERRAPHIM_DATA_PATH", self.root.path().join("data"));
        if let Some(home) = std::env::var_os("HOME") {
            command.env("HOME", home);
        }
        Ok(command)
    }
    fn server_command(&self, store: Option<&Path>) -> Result<Command> {
        let binary = std::env::var_os("TERRAPHIM_MCP_SERVER_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_terraphim_mcp_server").into());
        let mut command = self.command(&binary)?;
        command
            .args(["--profile", "server", "--config-dir"])
            .arg(&self.project);
        if let Some(store) = store {
            command.arg("--agent-data-dir").arg(store);
        }
        Ok(command)
    }
    async fn start(&self, store: Option<&Path>) -> Result<Client> {
        Ok(tokio::time::timeout(
            Duration::from_secs(20),
            ().serve(TokioChildProcess::new(self.server_command(store)?)?),
        )
        .await??)
    }
    async fn cli_output(&self, args: &[&str]) -> Result<std::process::Output> {
        // CI builds both workspace binaries before this test; missing CLI is a failure.
        let binary = std::env::var_os("TERRAPHIM_AGENT_TEST_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_BIN_EXE_terraphim_mcp_server"))
                    .parent()
                    .unwrap()
                    .join("terraphim-agent")
            });
        ensure!(
            binary.is_file(),
            "Build terraphim-agent before the MCP interoperability tests"
        );
        let output = self
            .command(&binary)?
            .env("TERRAPHIM_DEFAULT_DATA_PATH", &self.store)
            .args(["--format", "json"])
            .args(args)
            .output()
            .await?;
        Ok(output)
    }
    async fn cli(&self, args: &[&str]) -> Result<Value> {
        let output = self.cli_output(args).await?;
        ensure!(
            output.status.success(),
            "CLI failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(serde_json::from_slice(&output.stdout)?)
    }
}
fn evidence(label: &str, value: &Value) -> Result<()> {
    if let Some(directory) = std::env::var_os("TERRAPHIM_MCP_TEST_EVIDENCE") {
        fs::create_dir_all(&directory)?;
        fs::write(
            Path::new(&directory).join(format!("{label}.json")),
            serde_json::to_vec_pretty(value)?,
        )?;
    }
    Ok(())
}
async fn call(
    client: &rmcp::service::Peer<RoleClient>,
    name: &str,
    args: Value,
    label: &str,
) -> Result<Value> {
    let value = serde_json::to_value(
        tokio::time::timeout(
            Duration::from_secs(20),
            client.call_tool(CallToolRequestParam {
                name: name.to_owned().into(),
                arguments: args.as_object().cloned(),
            }),
        )
        .await??,
    )?;
    evidence(label, &json!({"tool":name,"arguments":args,"result":value}))?;
    Ok(value)
}
fn data(value: &Value) -> &Value {
    assert_ne!(value["isError"], true, "{value}");
    &value["structuredContent"]["data"]
}

#[tokio::test]
async fn tools_are_opt_in_with_complete_contracts_and_stdio_scope() -> Result<()> {
    let f = Fixture::new()?;
    let disabled = f.start(None).await?;
    let tools = disabled.list_all_tools().await?;
    assert!(!tools.iter().any(|tool| tool.name.starts_with("agent_")));
    assert!(
        disabled
            .call_tool(CallToolRequestParam {
                name: "agent_memory_capture".into(),
                arguments: Some(
                    json!({"content":"must not save"})
                        .as_object()
                        .unwrap()
                        .clone()
                )
            })
            .await
            .is_err()
    );
    assert!(!f.store.exists());
    disabled.cancel().await?;
    let enabled = f.start(Some(&f.store)).await?;
    let tools = enabled.list_all_tools().await?;
    let agent: Vec<_> = tools
        .iter()
        .filter(|tool| tool.name.starts_with("agent_"))
        .collect();
    assert_eq!(agent.len(), 9);
    for tool in &agent {
        assert_eq!(tool.input_schema["type"], "object");
        assert_eq!(tool.input_schema["additionalProperties"], false);
        assert_eq!(tool.output_schema.as_ref().unwrap()["type"], "object");
        let annotations = tool.annotations.as_ref().unwrap();
        assert_eq!(annotations.open_world_hint, Some(false));
        assert_eq!(
            annotations.read_only_hint,
            Some(matches!(
                tool.name.as_ref(),
                "agent_memory_show" | "agent_memory_list" | "agent_learning_query"
            ))
        );
    }
    evidence("agent-tools-list", &serde_json::to_value(agent)?)?;
    enabled.cancel().await?;
    let relative = f
        .server_command(Some(Path::new("relative")))?
        .output()
        .await?;
    assert!(!relative.status.success());
    assert!(String::from_utf8_lossy(&relative.stderr).contains("must be absolute"));
    let sse = f
        .server_command(Some(&f.store))?
        .arg("--sse")
        .output()
        .await?;
    assert!(!sse.status.success());
    assert!(String::from_utf8_lossy(&sse.stderr).contains("cannot be used"));
    Ok(())
}

#[tokio::test]
async fn authored_memory_survives_restart_and_cli_interoperability() -> Result<()> {
    let f = Fixture::new()?;
    let client = f.start(Some(&f.store)).await?;
    let initial = call(
        &client,
        "agent_memory_list",
        json!({}),
        "agent-memory-empty",
    )
    .await?;
    assert_eq!(data(&initial)["count"], 0);
    let content = "Risk requires a Decision: approve a second supplier. Café — Ελληνικά.\n";
    let captured = call(
        &client,
        "agent_memory_capture",
        json!({"content":content,"provenance_tag":"public-project-brief"}),
        "agent-memory-captured",
    )
    .await?;
    let id = data(&captured)["id"]
        .as_str()
        .context("saved id")?
        .to_owned();
    assert_eq!(data(&captured)["content"], content);
    assert_eq!(
        data(&captured)["tags"],
        json!(["provenance:public-project-brief"])
    );
    let path = PathBuf::from(
        captured["structuredContent"]["storage"]["memory_file"]
            .as_str()
            .unwrap(),
    );
    assert!(path.starts_with(f.store.canonicalize()?));
    let persisted: Value = serde_json::from_slice(&fs::read(&path)?)?;
    assert!(persisted.to_string().contains(&id));
    evidence("agent-memory-persisted", &persisted)?;
    client.cancel().await?;
    let restarted = f.start(Some(&f.store)).await?;
    let shown = call(
        &restarted,
        "agent_memory_show",
        json!({"id":id}),
        "agent-memory-restarted",
    )
    .await?;
    assert_eq!(data(&shown), data(&captured));
    let cli = f.cli(&["memory", "show", &id, "--json"]).await?;
    assert_eq!(cli["memory_item"]["content"], content);
    evidence("agent-memory-cli-readback", &cli)?;
    let from_cli = f
        .cli(&[
            "memory",
            "capture",
            "--content",
            "CLI authored second memory",
            "--provenance-tag",
            "cli-proof",
        ])
        .await?;
    let cli_id = from_cli["memory_id"].as_str().context("CLI saved id")?;
    let shown = call(
        &restarted,
        "agent_memory_show",
        json!({"id":cli_id}),
        "agent-memory-from-cli",
    )
    .await?;
    assert_eq!(data(&shown)["content"], "CLI authored second memory");
    assert_eq!(data(&shown)["tags"], json!(["provenance:cli-proof"]));
    let other = f.root.path().join("other store");
    let isolated = f.start(Some(&other)).await?;
    let missing = call(
        &isolated,
        "agent_memory_show",
        json!({"id":id}),
        "agent-memory-other-root",
    )
    .await?;
    assert_eq!(missing["isError"], true);
    assert!(!other.join("terraphim/evolution/cli-agent.json").exists());
    isolated.cancel().await?;
    restarted.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn actual_failure_annotation_explicit_rule_compile_and_synonym_export() -> Result<()> {
    let f = Fixture::new()?;
    let client = f.start(Some(&f.store)).await?;
    let failed = Command::new("printf").arg("%Z").output().await?;
    assert_eq!(failed.status.code(), Some(1));
    let error = String::from_utf8(failed.stderr)?;
    let corrected_command = r#"printf '%s\n' 'reviewed correction'"#;
    let corrected = Command::new("/bin/sh")
        .args(["-c", corrected_command])
        .output()
        .await?;
    assert!(corrected.status.success());
    assert_eq!(corrected.stdout, b"reviewed correction\n");
    evidence(
        "agent-learning-actual-execution",
        &json!({"failed":{"argv":["printf","%Z"],"exit":failed.status.code(),"stderr":error},"corrected":{"argv":["/bin/sh","-c",corrected_command],"exit":corrected.status.code(),"stdout":String::from_utf8(corrected.stdout)?}}),
    )?;
    let captured = call(
        &client,
        "agent_learning_capture",
        json!({"command":"printf '%Z'","error_output":error,"exit_code":1}),
        "agent-learning-captured",
    )
    .await?;
    let id = data(&captured)["id"].as_str().context("learning id")?;
    assert_eq!(
        data(&captured)["error_output"].as_str().unwrap().trim(),
        error.trim()
    );
    assert_eq!(data(&captured)["exit_code"], 1);
    let annotated = call(
        &client,
        "agent_learning_correct",
        json!({"id":id,"correction":corrected_command}),
        "agent-learning-annotated",
    )
    .await?;
    assert_eq!(data(&annotated)["correction"], corrected_command);
    let empty = call(
        &client,
        "agent_correction_compile",
        json!({}),
        "agent-learning-annotation-compile",
    )
    .await?;
    assert_eq!(data(&empty)["count"], 0);
    assert_eq!(data(&empty)["activated"], false);
    let added=call(&client,"agent_correction_add",json!({"original":"printf '%Z'","corrected":corrected_command,"context":"Actual public fixture"}),"agent-learning-explicit-rule").await?;
    assert_eq!(data(&added)["kind"], "correction");
    let compiled = call(
        &client,
        "agent_correction_compile",
        json!({}),
        "agent-learning-compiled",
    )
    .await?;
    assert_eq!(data(&compiled)["count"], 1);
    assert_eq!(data(&compiled)["activated"], false);
    assert_eq!(data(&compiled)["mappings"][0]["display"], corrected_command);
    let compiled_path = PathBuf::from(data(&compiled)["path"].as_str().unwrap());
    let disk: terraphim_types::Thesaurus = serde_json::from_slice(&fs::read(&compiled_path)?)?;
    assert_eq!(disk.len(), 1);
    assert_eq!(
        disk.get(&terraphim_types::NormalizedTermValue::from("printf '%Z'"))
            .unwrap()
            .display(),
        corrected_command
    );
    let stored_command = disk
        .get(&terraphim_types::NormalizedTermValue::from("printf '%Z'"))
        .unwrap()
        .display()
        .to_string();
    let stored_execution = Command::new("/bin/sh")
        .args(["-c", &stored_command])
        .current_dir(f.root.path())
        .output()
        .await?;
    assert!(stored_execution.status.success());
    assert_eq!(stored_execution.stdout, b"reviewed correction\n");
    let output_path = f.root.path().join("compiled-command.stdout");
    fs::write(&output_path, &stored_execution.stdout)?;
    assert_eq!(fs::read(&output_path)?, b"reviewed correction\n");
    evidence(
        "agent-learning-exact-compiled-execution",
        &json!({"compiledPath":compiled_path,"storedCommand":stored_command,"argv":["/bin/sh","-c",stored_command],"exit":stored_execution.status.code(),"stdout":String::from_utf8(stored_execution.stdout)?,"stderr":String::from_utf8(stored_execution.stderr)?,"savedOutput":String::from_utf8(fs::read(output_path)?)?}),
    )?;
    let exported = call(
        &client,
        "agent_correction_export_kg",
        json!({}),
        "agent-learning-exported",
    )
    .await?;
    assert_eq!(data(&exported)["registered"], false);
    let notes = data(&exported)["files"].as_array().unwrap();
    assert_eq!(notes.len(), 1);
    let note = fs::read_to_string(notes[0]["path"].as_str().unwrap())?;
    assert_eq!(note, notes[0]["markdown"].as_str().unwrap());
    assert!(note.contains("synonyms:: printf '%Z'"));
    evidence(
        "agent-learning-saved-artefacts",
        &json!({"compiled":serde_json::from_slice::<Value>(&fs::read(compiled_path)?)?,"note":note,"failureMarkdown":fs::read_to_string(data(&annotated)["path"].as_str().unwrap())?}),
    )?;
    client.cancel().await?;
    let restarted = f.start(Some(&f.store)).await?;
    let queried = call(
        &restarted,
        "agent_learning_query",
        json!({"query":"PRINTF","limit":10}),
        "agent-learning-restarted-query",
    )
    .await?;
    assert_eq!(data(&queried)["count"], 2);
    // CLI reads the same existing-format learning records under the explicit override.
    let cli = f.cli_output(&["learn", "query", "printf"]).await?;
    assert!(cli.status.success());
    let cli_text = String::from_utf8(cli.stdout)?;
    evidence(
        "agent-learning-cli-query",
        &json!({"exit":cli.status.code(),"stdout":cli_text}),
    )?;
    assert!(cli_text.contains("[cmd] printf '%Z' (exit: 1)"));
    assert!(cli_text.contains(corrected_command));
    let lock_path = f.store.join("terraphim/learnings/.write.lock");
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path)?;
    lock.lock()?;
    let failure_path = PathBuf::from(data(&annotated)["path"].as_str().unwrap());
    let before = fs::read(&failure_path)?;
    for (index, args) in [
        vec![
            "learn",
            "capture",
            "must-not-capture",
            "--error",
            "observed error",
        ],
        vec!["learn", "correct", id, "--correction", "must-not-replace"],
        vec![
            "learn",
            "correction",
            "add",
            "--original",
            "must-not-add",
            "--corrected",
            "replacement",
            "--correction-type",
            "tool-preference",
        ],
    ]
    .iter()
    .enumerate()
    {
        let refused = f.cli_output(args).await?;
        assert!(!refused.status.success());
        let stderr = String::from_utf8(refused.stderr)?;
        assert!(stderr.contains("busy"), "{stderr}");
        evidence(
            &format!("agent-learning-cli-busy-{index}"),
            &json!({"argv":args,"exit":refused.status.code(),"stderr":stderr}),
        )?;
    }
    assert_eq!(fs::read(&failure_path)?, before);
    drop(lock);
    let edited = f
        .cli_output(&[
            "learn",
            "correct",
            id,
            "--correction",
            "CLI reviewed annotation",
        ])
        .await?;
    assert!(edited.status.success());
    let actual = call(
        &restarted,
        "agent_learning_query",
        json!({"query":"printf"}),
        "agent-learning-cli-mutation-readback",
    )
    .await?;
    assert!(
        data(&actual)["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == id && item["correction"] == "CLI reviewed annotation")
    );
    assert!(!f.store.join(".claude").exists());
    assert!(!f.store.join(".opencode").exists());
    restarted.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn invalid_arguments_corrupt_storage_and_busy_writer_never_acknowledge_success() -> Result<()>
{
    let f = Fixture::new()?;
    let client = f.start(Some(&f.store)).await?;
    for args in [
        json!({"provenance_tag":"tag-only"}),
        json!({"content":"valid","unexpected":true}),
    ] {
        assert!(
            client
                .call_tool(CallToolRequestParam {
                    name: "agent_memory_capture".into(),
                    arguments: args.as_object().cloned()
                })
                .await
                .is_err()
        );
    }
    for args in [
        json!({"limit":-1}),
        serde_json::from_str("{\"limit\":18446744073709551616}")?,
        json!({"limit":1.5}),
    ] {
        assert!(
            client
                .call_tool(CallToolRequestParam {
                    name: "agent_memory_list".into(),
                    arguments: args.as_object().cloned()
                })
                .await
                .is_err()
        );
    }
    for (index, (name, args)) in [
        ("agent_memory_capture", json!({"content":" \n"})),
        ("agent_memory_show", json!({"id":"../escape"})),
        ("agent_memory_show", json!({"id":"missing"})),
        ("agent_memory_list", json!({"limit":0})),
        (
            "agent_learning_capture",
            json!({"command":"printf ok","error_output":"none","exit_code":0}),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            call(
                &client,
                name,
                args,
                &format!("agent-rejected-input-{index}")
            )
            .await?["isError"],
            true
        );
    }
    let capture = call(
        &client,
        "agent_memory_capture",
        json!({"content":"Preserve this actual note"}),
        "agent-before-storage-failures",
    )
    .await?;
    let path = PathBuf::from(
        capture["structuredContent"]["storage"]["memory_file"]
            .as_str()
            .unwrap(),
    );
    let before = fs::read(&path)?;
    let lock_path = path.parent().unwrap().join("cli-agent.lock");
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path)?;
    lock.lock()?;
    let busy = call(
        &client,
        "agent_memory_capture",
        json!({"content":"Must not acknowledge under lock"}),
        "agent-busy-refused",
    )
    .await?;
    assert_eq!(busy["isError"], true);
    let cli_busy = f
        .cli_output(&[
            "memory",
            "capture",
            "--content",
            "Must not override held memory lock",
        ])
        .await?;
    assert!(!cli_busy.status.success());
    assert!(String::from_utf8_lossy(&cli_busy.stderr).contains("busy"));
    evidence(
        "agent-memory-cli-busy",
        &json!({"exit":cli_busy.status.code(),"stdout":String::from_utf8(cli_busy.stdout)?,"stderr":String::from_utf8(cli_busy.stderr)?}),
    )?;
    assert_eq!(fs::read(&path)?, before);
    drop(lock);
    fs::write(&path, b"{ corrupt retained")?;
    let refused = call(
        &client,
        "agent_memory_capture",
        json!({"content":"Must not erase corruption"}),
        "agent-corrupt-refused",
    )
    .await?;
    assert_eq!(refused["isError"], true);
    assert_eq!(fs::read(&path)?, b"{ corrupt retained");
    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn concurrent_mcp_captures_keep_every_acknowledged_record() -> Result<()> {
    let f = Fixture::new()?;
    let client = f.start(Some(&f.store)).await?;
    let mut calls = tokio::task::JoinSet::new();
    for i in 0..8 {
        let peer = client.peer().clone();
        calls.spawn(async move {
            call(
                &peer,
                "agent_memory_capture",
                json!({"content":format!("Concurrent public note {i}")}),
                &format!("agent-concurrent-{i}"),
            )
            .await
        });
    }
    let mut results = Vec::new();
    while let Some(result) = calls.join_next().await {
        results.push(result?);
    }
    let mut ids = Vec::new();
    for result in results {
        let value = result?;
        ids.push(data(&value)["id"].as_str().unwrap().to_owned());
    }
    let listed = call(
        &client,
        "agent_memory_list",
        json!({"limit":100}),
        "agent-concurrent-list",
    )
    .await?;
    assert_eq!(data(&listed)["count"], 8);
    for id in ids {
        assert!(
            data(&listed)["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["id"] == id)
        );
    }
    client.cancel().await?;
    Ok(())
}
