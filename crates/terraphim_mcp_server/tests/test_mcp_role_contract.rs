//! Real stdio regressions for authored synonyms and role-bound autocomplete.
use anyhow::{Context, Result};
use rmcp::{
    RoleClient,
    model::CallToolRequestParam,
    service::{RunningService, ServiceExt},
    transport::TokioChildProcess,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, time::Duration};
use tempfile::TempDir;
use tokio::process::Command;

struct Fixture {
    root: TempDir,
    client: RunningService<RoleClient, ()>,
}
impl Fixture {
    async fn start() -> Result<Self> {
        Self::with_fuzzy_candidate(false).await
    }
    async fn with_fuzzy_candidate(include_icon: bool) -> Result<Self> {
        let root = tempfile::tempdir()?;
        let project = root.path().join("project with spaces");
        let settings = root.path().join("settings");
        fs::create_dir_all(project.join(".terraphim"))?;
        fs::create_dir_all(&settings)?;
        let mut roles = serde_json::Map::new();
        for (name, canonical, alias) in [
            ("project", "Risk", "Concern"),
            ("garden", "Orchid", "Flower"),
        ] {
            let concepts = project.join(name).join("concepts");
            let corpus = project.join(name).join("corpus");
            fs::create_dir_all(&concepts)?;
            fs::create_dir_all(&corpus)?;
            fs::write(
                concepts.join(format!("{canonical}.md")),
                format!("# {canonical}\nsynonyms:: {alias}\n"),
            )?;
            for (i, suffix) in [
                "requires a decision.",
                "requires a mitigation.",
                "requires a review.",
            ]
            .iter()
            .enumerate()
            {
                fs::write(
                    corpus.join(format!("decision-{i}.md")),
                    format!("{canonical} {suffix}\n"),
                )?;
            }
            if include_icon && name == "project" {
                fs::write(concepts.join("Icon.md"), "# Icon\nsynonyms:: Pictogram\n")?;
                fs::write(
                    corpus.join("icon.md"),
                    "Icon identifies the review action.\n",
                )?;
            }
            roles.insert(name.into(), json!({"name":name,"relevance_function":"terraphim-graph","terraphim_it":false,"theme":"lumen","kg":{"automata_path":null,"knowledge_graph_local":{"input_type":"markdown","path":concepts},"public":false,"publish":false},"haystacks":[{"location":corpus,"service":"Ripgrep","read_only":true,"extra_parameters":{}}],"extra":{}}));
        }
        // Replace the profile's remote-backed defaults with local, empty roles.
        for name in ["Default", "Engineer", "System Operator"] {
            roles.insert(name.into(), json!({"name":name,"relevance_function":"title-scorer","terraphim_it":false,"theme":"lumen","kg":null,"haystacks":[],"extra":{}}));
        }
        fs::write(
            project.join(".terraphim/config.json"),
            serde_json::to_vec_pretty(
                &json!({"selected_role":"project","default_role":"project","roles":roles}),
            )?,
        )?;
        fs::write(
            settings.join("settings.toml"),
            format!(
                "server_hostname = \"127.0.0.1:8000\"\napi_endpoint = \"http://localhost:8000/api\"\ninitialized = true\ndefault_data_path = {}\n[profiles.memory]\ntype = \"memory\"\n",
                serde_json::to_string(&root.path().join("data"))?
            ),
        )?;
        let binary = std::env::var_os("TERRAPHIM_MCP_SERVER_BIN")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_terraphim_mcp_server").into());
        let mut command = Command::new(binary);
        command
            .args(["--profile", "server", "--config-dir"])
            .arg(&project)
            .current_dir(&project)
            .kill_on_drop(true);
        // No HOME override or inherited credentials. Preserve only executable discovery.
        let path = std::env::var_os("PATH").context("PATH")?;
        command
            .env_clear()
            .env("PATH", path)
            .env("TERRAPHIM_SETTINGS_PATH", &settings)
            .env("TERRAPHIM_DATA_PATH", root.path().join("data"));
        if let Some(home) = std::env::var_os("HOME") {
            command.env("HOME", home);
        }
        let client = tokio::time::timeout(
            Duration::from_secs(20),
            ().serve(TokioChildProcess::new(command)?),
        )
        .await??;
        Ok(Self { root, client })
    }
    async fn call(&self, name: &str, args: Value, evidence: &str) -> Result<Value> {
        let result = tokio::time::timeout(
            Duration::from_secs(20),
            self.client.call_tool(CallToolRequestParam {
                name: name.to_owned().into(),
                arguments: args.as_object().cloned(),
            }),
        )
        .await??;
        let value = serde_json::to_value(result)?;
        if let Some(dir) = std::env::var_os("TERRAPHIM_MCP_TEST_EVIDENCE") {
            fs::create_dir_all(&dir)?;
            fs::write(
                std::path::Path::new(&dir).join(format!("{evidence}.json")),
                serde_json::to_vec_pretty(&json!({"tool":name,"arguments":args,"result":value}))?,
            )?;
        }
        Ok(value)
    }
}
fn resources(value: &Value) -> BTreeMap<String, String> {
    value["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| {
            let resource = item.get("resource")?;
            Some((
                resource["uri"].as_str()?.to_owned(),
                resource["text"].as_str()?.to_owned(),
            ))
        })
        .collect()
}
fn texts(value: &Value) -> String {
    value["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn authored_synonym_retrieves_actual_canonical_documents() -> Result<()> {
    let f = Fixture::start().await?;
    let canonical = f
        .call(
            "search",
            json!({"query":"Risk","role":"project","limit":20}),
            "synonym-canonical",
        )
        .await?;
    let synonym = f
        .call(
            "search",
            json!({"query":"Concern","role":"project","limit":20}),
            "synonym-alias",
        )
        .await?;
    let expected = resources(&canonical);
    assert_eq!(expected.len(), 3, "canonical retrieval: {canonical}");
    for text in expected.values() {
        assert!(text.contains("Risk requires"));
        assert!(!text.contains("Concern"));
    }
    assert_eq!(
        resources(&synonym),
        expected,
        "authored synonym must retrieve the same saved documents: {synonym}"
    );
    let baseline = f
        .call(
            "search",
            json!({"query":"Unrelated","role":"project"}),
            "synonym-unrelated",
        )
        .await?;
    assert!(resources(&baseline).is_empty());
    // Keep the fixture alive until the child is cancelled.
    assert!(f.root.path().exists());
    f.client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn autocomplete_refuses_unknown_roles() -> Result<()> {
    let f = Fixture::start().await?;
    f.call(
        "build_autocomplete_index",
        json!({"role":"project"}),
        "unknown-built",
    )
    .await?;
    for tool in ["autocomplete_terms", "autocomplete_with_snippets"] {
        let value = f
            .call(
                tool,
                json!({"query":"Con","role":"missing-role","limit":8}),
                &format!("unknown-{tool}"),
            )
            .await?;
        assert_eq!(
            value["isError"], true,
            "unknown role must not use another role's index: {value}"
        );
        assert!(!texts(&value).contains("\nConcern"));
    }
    f.client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn autocomplete_retains_disjoint_role_indexes_and_selected_role() -> Result<()> {
    let f = Fixture::start().await?;
    for role in ["project", "garden"] {
        let result = f
            .call(
                "build_autocomplete_index",
                json!({"role":role}),
                &format!("switch-build-{role}"),
            )
            .await?;
        assert_ne!(result["isError"], true, "{result}");
    }
    for (role, query, expected, forbidden) in [
        ("project", "Con", "Concern", "Flower"),
        ("garden", "Flo", "Flower", "Concern"),
        ("project", "Con", "Concern", "Flower"),
    ] {
        for tool in ["autocomplete_terms", "autocomplete_with_snippets"] {
            let result = f
                .call(
                    tool,
                    json!({"query":query,"role":role,"limit":8}),
                    &format!("switch-{role}-{tool}"),
                )
                .await?;
            assert_ne!(result["isError"], true, "{result}");
            assert!(
                texts(&result)
                    .to_lowercase()
                    .contains(&expected.to_lowercase()),
                "{result}"
            );
            assert!(
                !texts(&result)
                    .to_lowercase()
                    .contains(&forbidden.to_lowercase()),
                "{result}"
            );
        }
    }
    let selected = f
        .call(
            "autocomplete_terms",
            json!({"query":"Con","limit":8}),
            "switch-selected",
        )
        .await?;
    assert!(
        texts(&selected).contains("concern"),
        "omitted role must use selected project role: {selected}"
    );
    f.client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn synonym_union_deduplicates_before_pagination() -> Result<()> {
    let f = Fixture::start().await?;
    fs::write(
        f.root
            .path()
            .join("project with spaces/project/corpus/both.md"),
        "Risk and Concern refer to the same event.\n",
    )?;
    let all = f
        .call(
            "search",
            json!({"query":"Concern","role":"project"}),
            "pages-all",
        )
        .await?;
    assert_eq!(resources(&all).len(), 4, "{all}");
    let mut pages = BTreeMap::new();
    for skip in 0..4 {
        let alias = f
            .call(
                "search",
                json!({"query":"Concern","role":"project","limit":1,"skip":skip}),
                &format!("pages-alias-{skip}"),
            )
            .await?;
        let canonical = f
            .call(
                "search",
                json!({"query":"Risk","role":"project","limit":1,"skip":skip}),
                &format!("pages-canonical-{skip}"),
            )
            .await?;
        assert_eq!(resources(&alias), resources(&canonical));
        assert_eq!(resources(&alias).len(), 1);
        pages.extend(resources(&alias));
    }
    assert_eq!(pages, resources(&all));
    for (name, args) in [
        (
            "zero",
            json!({"query":"Concern","role":"project","limit":0}),
        ),
        (
            "exhausted",
            json!({"query":"Concern","role":"project","skip":4}),
        ),
        (
            "substring",
            json!({"query":"UnrelatedConcern","role":"project"}),
        ),
        (
            "compound",
            json!({"query":"Concern requires","role":"project"}),
        ),
    ] {
        let result = f.call("search", args, &format!("pages-{name}")).await?;
        assert!(resources(&result).is_empty(), "{name}: {result}");
    }
    for args in [
        json!({"query":"Concern","role":"project","limit":-1}),
        json!({"query":"Concern","role":"project","skip":-1}),
        json!({"query":"Concern","role":"missing-role"}),
    ] {
        let result = f
            .client
            .call_tool(CallToolRequestParam {
                name: "search".into(),
                arguments: args.as_object().cloned(),
            })
            .await;
        if args["role"] == "missing-role" {
            assert_eq!(result?.is_error, Some(true));
        } else {
            assert!(
                result.is_err(),
                "invalid pagination must fail before execution"
            );
        }
    }
    f.client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn autocomplete_snippets_use_the_requested_role_haystack() -> Result<()> {
    let f = Fixture::start().await?;
    f.call(
        "build_autocomplete_index",
        json!({"role":"garden"}),
        "snippet-build",
    )
    .await?;
    let result = f
        .call(
            "autocomplete_with_snippets",
            json!({"query":"Flo","role":"garden","limit":2}),
            "snippet-garden",
        )
        .await?;
    assert_ne!(result["isError"], true, "{result}");
    assert!(texts(&result).contains("Orchid requires"), "{result}");
    assert!(!texts(&result).contains("Risk requires"), "{result}");
    f.client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn deserialised_index_cannot_impersonate_a_role_and_config_changes_invalidate_cache()
-> Result<()> {
    let f = Fixture::start().await?;
    for role in ["project", "garden"] {
        f.call(
            "build_autocomplete_index",
            json!({"role":role}),
            &format!("invalidate-build-{role}"),
        )
        .await?;
    }
    let serialised = f
        .call(
            "serialize_autocomplete_index",
            json!({}),
            "invalidate-serialise",
        )
        .await?;
    let encoded = serialised["content"][2]["text"]
        .as_str()
        .context("encoded index")?;
    let loaded = f
        .call(
            "deserialize_autocomplete_index",
            json!({"base64_data":encoded}),
            "invalidate-deserialise",
        )
        .await?;
    assert_ne!(loaded["isError"], true, "{loaded}");
    let project = f
        .call(
            "autocomplete_terms",
            json!({"query":"Con","role":"project"}),
            "invalidate-project-unchanged",
        )
        .await?;
    assert!(texts(&project).contains("concern"), "{project}");
    let mut config: Value = serde_json::from_slice(&fs::read(
        f.root
            .path()
            .join("project with spaces/.terraphim/config.json"),
    )?)?;
    config["id"] = json!("Server");
    config["roles"]["project"]["kg"] = config["roles"]["garden"]["kg"].clone();
    config["roles"].as_object_mut().unwrap().remove("garden");
    let updated = f
        .call(
            "update_config_tool",
            json!({"config_str":config.to_string()}),
            "invalidate-config",
        )
        .await?;
    assert_ne!(updated["isError"], true, "{updated}");
    for role in ["project", "garden"] {
        let result = f
            .call(
                "autocomplete_terms",
                json!({"query":"Con","role":role}),
                &format!("invalidate-{role}-refused"),
            )
            .await?;
        assert_eq!(result["isError"], true, "{result}");
    }
    f.call(
        "build_autocomplete_index",
        json!({"role":"project"}),
        "invalidate-rebuild",
    )
    .await?;
    let new = f
        .call(
            "autocomplete_terms",
            json!({"query":"Flo","role":"project"}),
            "invalidate-new-graph",
        )
        .await?;
    assert!(texts(&new).contains("flower"), "{new}");
    assert!(!texts(&new).contains("concern"), "{new}");
    f.client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn pagination_schema_is_enforced_without_integer_wrapping() -> Result<()> {
    let f = Fixture::start().await?;
    let tools = f.client.list_tools(Default::default()).await?;
    let search = tools
        .tools
        .iter()
        .find(|tool| tool.name == "search")
        .context("search tool")?;
    assert_eq!(
        search.input_schema["properties"]["limit"]["maximum"],
        i32::MAX
    );
    for field in ["limit", "skip"] {
        for value in [
            json!(4294967296_u64),
            json!(i64::MAX),
            json!(1.5),
            json!("2"),
            json!(-1),
            json!(null),
        ] {
            let mut args = json!({"query":"Concern","role":"project"});
            args[field] = value;
            let result = f
                .client
                .call_tool(CallToolRequestParam {
                    name: "search".into(),
                    arguments: args.as_object().cloned(),
                })
                .await;
            assert!(
                result.is_err(),
                "{field} accepted invalid arguments: {args}"
            );
        }
    }
    f.client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn fuzzy_autocomplete_returns_authored_terms_from_the_real_index() -> Result<()> {
    let f = Fixture::start().await?;
    let built = f
        .call(
            "build_autocomplete_index",
            json!({"role":"project"}),
            "fuzzy-built",
        )
        .await?;
    assert_ne!(built["isError"], true);
    for (tool, arguments) in [
        (
            "fuzzy_autocomplete_search",
            json!({"query":"concrn","similarity":0.8,"limit":8}),
        ),
        (
            "fuzzy_autocomplete_search_jaro_winkler",
            json!({"query":"concrn","similarity":0.8,"limit":8}),
        ),
        (
            "fuzzy_autocomplete_search_levenshtein",
            json!({"query":"concrn","max_edit_distance":1,"limit":8}),
        ),
    ] {
        let result = f.call(tool, arguments, tool).await?;
        assert_ne!(result["isError"], true, "{result}");
        let suggestions = result["content"]
            .as_array()
            .unwrap()
            .iter()
            .skip(1)
            .filter_map(|item| item["text"].as_str())
            .collect::<Vec<_>>();
        assert!(
            suggestions
                .iter()
                .any(|value| value.to_lowercase().contains("concern")),
            "{result}"
        );
        assert!(
            suggestions
                .iter()
                .all(|value| !value.to_lowercase().contains("flower")),
            "{result}"
        );
    }
    let absent = f
        .call(
            "fuzzy_autocomplete_search",
            json!({"query":"zzzzzz","similarity":1.0,"limit":8}),
            "fuzzy-absent",
        )
        .await?;
    assert_ne!(absent["isError"], true);
    assert_eq!(absent["content"].as_array().unwrap().len(), 1, "{absent}");
    f.client.cancel().await?;
    Ok(())
}

async fn check_unique_aliases_after_rebuild(tool: &str) -> Result<()> {
    let f = Fixture::start().await?;
    for (cycle, role) in ["project", "garden", "project"].iter().enumerate() {
        f.call(
            "build_autocomplete_index",
            json!({"role":role}),
            &format!("dedup-build-{tool}-{cycle}"),
        )
        .await?;
        let query = if *role == "project" { "Con" } else { "Flo" };
        let alias = if *role == "project" {
            "concern"
        } else {
            "flower"
        };
        let canonical = if *role == "project" { "risk" } else { "orchid" };
        for limit in [0, 1, 2, 5] {
            let result = f
                .call(
                    tool,
                    json!({"query":query,"role":role,"limit":limit}),
                    &format!("dedup-{cycle}-{tool}-{limit}"),
                )
                .await?;
            assert_ne!(result["isError"], true, "{result}");
            let items: Vec<_> = result["content"]
                .as_array()
                .unwrap()
                .iter()
                .skip(1)
                .map(|x| x["text"].as_str().unwrap())
                .collect();
            let unique: std::collections::HashSet<_> = items.iter().copied().collect();
            assert_eq!(unique.len(), items.len(), "duplicate suggestions: {result}");
            let count = if tool == "autocomplete_terms" {
                limit.min(2)
            } else {
                limit.min(1)
            };
            assert_eq!(
                items.len(),
                count,
                "distinct aliases must fill their available slots: {result}"
            );
            assert_eq!(
                result["content"][0]["text"],
                format!("Found {count} suggestions")
            );
            if limit > 0 {
                assert!(items[0].starts_with(alias), "{result}");
            }
            if tool == "autocomplete_terms" && limit >= 2 {
                assert!(
                    items.contains(&canonical),
                    "same-concept alias must remain distinct: {result}"
                );
            }
        }
    }
    f.client.cancel().await?;
    Ok(())
}

async fn check_distinct_fuzzy_slot(tool: &str) -> Result<()> {
    let f = Fixture::with_fuzzy_candidate(true).await?;
    f.call(
        "build_autocomplete_index",
        json!({"role":"project"}),
        &format!("dedup-fuzzy-build-{tool}"),
    )
    .await?;
    // The real fuzzy index must contain the distinct candidate before checking
    // that the combined autocomplete endpoints preserve its available slot.
    let fuzzy = f
        .call(
            "fuzzy_autocomplete_search",
            json!({"query":"con","similarity":0.6,"limit":2}),
            &format!("dedup-fuzzy-available-{tool}"),
        )
        .await?;
    assert!(texts(&fuzzy).contains("icon"), "{fuzzy}");
    let result = f
        .call(
            tool,
            json!({"query":"con","role":"project","limit":2}),
            &format!("dedup-fuzzy-{tool}"),
        )
        .await?;
    assert_ne!(result["isError"], true, "{result}");
    let items: Vec<_> = result["content"]
        .as_array()
        .unwrap()
        .iter()
        .skip(1)
        .map(|x| x["text"].as_str().unwrap())
        .collect();
    assert_eq!(items.len(), 2, "{result}");
    assert!(
        items[0].starts_with("concern"),
        "prefix remains first: {result}"
    );
    assert!(
        items[1].starts_with("icon"),
        "duplicate prefix must not consume the fuzzy slot: {result}"
    );
    if tool == "autocomplete_with_snippets" {
        assert!(items[0].contains("Risk requires"), "{result}");
        assert!(
            items[1].contains("Icon identifies the review action."),
            "{result}"
        );
    }
    assert_eq!(result["content"][0]["text"], "Found 2 suggestions");
    f.client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn autocomplete_terms_deduplicates_authored_aliases_after_rebuild() -> Result<()> {
    check_unique_aliases_after_rebuild("autocomplete_terms").await
}
#[tokio::test]
async fn autocomplete_snippets_deduplicates_authored_aliases_after_rebuild() -> Result<()> {
    check_unique_aliases_after_rebuild("autocomplete_with_snippets").await
}
#[tokio::test]
async fn autocomplete_terms_deduplicates_before_limit() -> Result<()> {
    check_distinct_fuzzy_slot("autocomplete_terms").await
}
#[tokio::test]
async fn autocomplete_snippets_deduplicates_before_limit() -> Result<()> {
    check_distinct_fuzzy_slot("autocomplete_with_snippets").await
}
