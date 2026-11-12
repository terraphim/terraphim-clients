use anyhow::Result;
use reqwest::header::{AUTHORIZATION, ETAG, HeaderMap, HeaderValue, IF_MATCH};
use reqwest::{Client, RequestBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use terraphim_types::{Document, SearchQuery};

/// The environment variable that carries the API key of the config plane.
///
/// The server authenticates its config endpoints with a bearer key. In
/// loopback developer mode no key is needed, so the variable is optional.
pub const API_KEY_ENV: &str = "TERRAPHIM_SERVER_API_KEY";

#[derive(Clone, Debug)]
pub struct ApiClient {
    http: Client,
    base: String,
}

/// The server's configuration together with the ETag that identifies it.
///
/// The ETag is what the config plane uses for optimistic concurrency: a write
/// that carries the ETag of a configuration that has changed since is rejected
/// (412) rather than overwriting somebody else's change.
#[derive(Debug, Clone)]
pub struct ConfigSnapshot {
    /// The configuration as the server holds it right now.
    pub config: terraphim_config::Config,
    /// The `ETag` of the response, when the server sent one.
    pub etag: Option<String>,
}

impl ApiClient {
    /// Build a client for the server at `base_url`.
    ///
    /// The API key is read from [`API_KEY_ENV`] when it is set. In loopback
    /// developer mode the server needs no key, so an unset variable is not an
    /// error.
    pub fn new(base_url: impl Into<String>) -> Self {
        let api_key = std::env::var(API_KEY_ENV)
            .ok()
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty());
        Self::build(base_url.into(), api_key.as_deref())
    }

    /// Build a client that authenticates with `api_key`.
    ///
    /// The key is sent as `Authorization: Bearer <key>` on every request, so
    /// it reaches the config endpoints that require it.
    pub fn with_api_key(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        let api_key = api_key.into();
        Self::build(base_url.into(), Some(api_key.as_str()))
    }

    fn build(base: String, api_key: Option<&str>) -> Self {
        // Use longer timeout for CI/CD environments where search may be slow
        let timeout_secs: u64 = match std::env::var("TERRAPHIM_CLIENT_TIMEOUT") {
            Ok(v) => v.parse().unwrap_or(60),
            Err(_) => 30,
        };
        let mut builder = Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .user_agent("Terraphim-TUI/1.0");

        if let Some(key) = api_key {
            let mut headers = HeaderMap::new();
            if let Ok(value) = HeaderValue::from_str(&format!("Bearer {key}")) {
                headers.insert(AUTHORIZATION, value);
            }
            builder = builder.default_headers(headers);
        }

        let client = builder.build().unwrap_or_else(|_| Client::new());

        Self { http: client, base }
    }

    pub async fn health(&self) -> Result<()> {
        let url = format!("{}/health", self.base);
        let res = self.http.get(url).send().await?;
        if res.status().is_success() {
            Ok(())
        } else {
            anyhow::bail!("health check failed")
        }
    }

    pub async fn get_config(&self) -> Result<ConfigResponse> {
        Ok(self.fetch_config().await?.0)
    }

    /// Fetch the configuration together with the ETag that identifies it.
    ///
    /// The ETag is what a write has to carry as `If-Match`, so this is the
    /// read every write is based on.
    pub async fn get_config_snapshot(&self) -> Result<ConfigSnapshot> {
        let (response, etag) = self.fetch_config().await?;
        Ok(ConfigSnapshot {
            config: response.config,
            etag,
        })
    }

    /// Read `/config` and keep both the body and the `ETag` header.
    async fn fetch_config(&self) -> Result<(ConfigResponse, Option<String>)> {
        let url = format!("{}/config", self.base);
        let res = self.http.get(url).send().await?;
        let etag = res
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let body = res.error_for_status()?.json::<ConfigResponse>().await?;
        Ok((body, etag))
    }

    /// Resolve a role string (name or shortname) to a RoleName using server config.
    /// Falls back to RoleName::new if no match found (server will validate).
    pub async fn resolve_role(&self, role: &str) -> Result<terraphim_types::RoleName> {
        use terraphim_types::RoleName;
        let config_res = self.get_config().await?;
        let role_lower = role.to_lowercase();
        let selected = &config_res.config.selected_role;
        if selected.to_string().to_lowercase() == role_lower {
            return Ok(selected.clone());
        }
        for (name, role_cfg) in &config_res.config.roles {
            if name.to_string().to_lowercase() == role_lower {
                return Ok(name.clone());
            }
            if let Some(ref sn) = role_cfg.shortname
                && sn.to_lowercase() == role_lower
            {
                return Ok(name.clone());
            }
        }
        Ok(RoleName::new(role))
    }

    /// Switch the server's selected role.
    ///
    /// `etag` is the ETag of the configuration the change is based on. The
    /// server rejects the write when the configuration changed since (412);
    /// see [`Self::send_config_write`].
    pub async fn update_selected_role(
        &self,
        role: &str,
        etag: Option<&str>,
    ) -> Result<ConfigResponse> {
        let url = format!("{}/config/selected_role", self.base);
        #[derive(Serialize)]
        struct Payload<'a> {
            selected_role: &'a str,
        }
        self.send_config_write(etag, |etag| {
            let request = self.http.post(&url).json(&Payload {
                selected_role: role,
            });
            match etag {
                Some(etag) => request.header(IF_MATCH, etag),
                None => request,
            }
        })
        .await
    }

    /// Switch the server's selected role.
    ///
    /// This is the shape every caller wants: read the configuration, then
    /// write with the ETag of that configuration as `If-Match`. A write that
    /// is based on a configuration that has changed since is reported, not
    /// retried.
    pub async fn switch_role(&self, role: &str) -> Result<ConfigResponse> {
        let snapshot = self.get_config_snapshot().await?;
        self.update_selected_role(role, snapshot.etag.as_deref())
            .await
    }

    /// Write the configuration back to the server.
    ///
    /// The configuration is sent unchanged - including the values the server
    /// redacted, which are sent back as they were received so the server can
    /// keep the values it holds.
    ///
    /// `etag` is the ETag of the configuration the write is based on; see
    /// [`Self::send_config_write`].
    pub async fn post_config(
        &self,
        cfg: &terraphim_config::Config,
        etag: Option<&str>,
    ) -> Result<ConfigResponse> {
        let url = format!("{}/config", self.base);
        self.send_config_write(etag, |etag| {
            let request = self.http.post(&url).json(cfg);
            match etag {
                Some(etag) => request.header(IF_MATCH, etag),
                None => request,
            }
        })
        .await
    }

    /// Send a write to the config plane and interpret its answer.
    ///
    /// The config plane protects writes with `If-Match`: a write that carries
    /// the ETag of a configuration that has changed since is rejected with 412.
    /// The write is **not** retried - the caller is told that the
    /// configuration changed elsewhere, because retrying would apply a change
    /// that was computed against a configuration that no longer exists.
    async fn send_config_write(
        &self,
        etag: Option<&str>,
        build: impl FnOnce(Option<&str>) -> RequestBuilder,
    ) -> Result<ConfigResponse> {
        let res = build(etag).send().await?;

        if res.status() == StatusCode::PRECONDITION_FAILED {
            // Fetch the configuration once more, so the report can say what it
            // looks like now, and then report the conflict.
            let current = self.get_config_snapshot().await.ok();
            let selected = current.map(|snapshot| snapshot.config.selected_role.to_string());
            anyhow::bail!(
                "the configuration changed elsewhere: the server rejected the write (412 \
                 precondition failed){} - the write was not applied, read the configuration \
                 again and re-apply the change",
                selected
                    .map(|role| format!("; the selected role is now '{role}'"))
                    .unwrap_or_default()
            );
        }

        if res.status() == StatusCode::PRECONDITION_REQUIRED {
            anyhow::bail!(
                "the server requires If-Match on configuration writes but did not return an \
                 ETag - the client and the server are out of step"
            );
        }

        let body = res.error_for_status()?.json::<ConfigResponse>().await?;
        Ok(body)
    }

    pub async fn get_rolegraph_edges(&self, role: Option<&str>) -> Result<RoleGraphResponseDto> {
        self.rolegraph(role).await
    }

    pub async fn search(&self, query: &SearchQuery) -> Result<SearchResponse> {
        let url = format!("{}/documents/search", self.base);
        let res = self.http.post(url).json(query).send().await?;
        let body = res.error_for_status()?.json::<SearchResponse>().await?;
        Ok(body)
    }

    pub async fn rolegraph(&self, role: Option<&str>) -> Result<RoleGraphResponseDto> {
        let url = match role {
            Some(r) if !r.is_empty() => {
                format!("{}/rolegraph?role={}", self.base, urlencoding::encode(r))
            }
            _ => format!("{}/rolegraph", self.base),
        };
        let res = self.http.get(url).send().await?;
        let body = res
            .error_for_status()?
            .json::<RoleGraphResponseDto>()
            .await?;
        Ok(body)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchResponse {
    pub status: String,
    pub results: Vec<Document>,
    pub total: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConfigResponse {
    pub status: String,
    pub config: terraphim_config::Config,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GraphNodeDto {
    pub id: u64,
    pub label: String,
    pub rank: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GraphEdgeDto {
    pub source: u64,
    pub target: u64,
    pub rank: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RoleGraphResponseDto {
    pub status: String,
    pub nodes: Vec<GraphNodeDto>,
    pub edges: Vec<GraphEdgeDto>,
    #[serde(default)]
    pub pinned_node_ids: Vec<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatRequest {
    pub role: String,
    pub messages: Vec<ChatMessage>,
    pub model: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatResponse {
    pub status: String,
    pub message: Option<String>,
    pub model_used: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SummarizeRequest {
    pub document: Document,
    pub role: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SummarizeResponse {
    pub status: String,
    pub summary: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ThesaurusResponse {
    pub status: String,
    pub thesaurus: Option<std::collections::HashMap<String, String>>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AutocompleteSuggestion {
    pub text: String,
    pub score: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AutocompleteResponse {
    pub status: String,
    pub suggestions: Vec<AutocompleteSuggestion>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AsyncSummarizeResponse {
    pub status: String,
    pub task_id: String,
    pub message: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TaskStatusResponse {
    pub status: String,
    pub task_id: String,
    pub state: String, // "pending", "processing", "completed", "failed", "cancelled"
    pub progress: Option<f64>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct QueueStatsResponse {
    pub status: String,
    pub pending_tasks: usize,
    pub processing_tasks: usize,
    pub completed_tasks: usize,
    pub failed_tasks: usize,
    pub total_tasks: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BatchSummarizeRequest {
    pub documents: Vec<Document>,
    pub role: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BatchSummarizeResponse {
    pub status: String,
    pub task_ids: Vec<String>,
    pub message: Option<String>,
    pub error: Option<String>,
}

// VM Management Types

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VmWithIp {
    pub vm_id: String,
    pub ip_address: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VmPoolListResponse {
    pub vms: Vec<VmWithIp>,
    pub stats: VmPoolStatsResponse,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VmPoolStatsResponse {
    pub total_ips: usize,
    pub allocated_ips: usize,
    pub available_ips: usize,
    pub utilization_percent: u8,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VmStatusResponse {
    pub vm_id: String,
    pub status: String,
    pub ip_address: String,
    pub created_at: String,
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VmExecuteRequest {
    pub code: String,
    pub language: String,
    pub agent_id: String,
    pub vm_id: Option<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VmExecuteResponse {
    pub execution_id: String,
    pub vm_id: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub started_at: String,
    pub completed_at: String,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VmTask {
    pub id: String,
    pub vm_id: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VmTasksResponse {
    pub tasks: Vec<VmTask>,
    pub vm_id: String,
    pub total: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VmAllocateRequest {
    pub vm_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VmAllocateResponse {
    pub vm_id: String,
    pub ip_address: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VmMetricsResponse {
    pub vm_id: String,
    pub status: String,
    pub cpu_usage_percent: f64,
    pub memory_usage_mb: u32,
    pub disk_usage_percent: f64,
    pub network_io_mbps: f64,
    pub uptime_seconds: u64,
    pub process_count: u32,
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VmAgentRequest {
    pub agent_id: String,
    pub task: String,
    pub vm_id: Option<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VmAgentResponse {
    pub task_id: String,
    pub agent_id: String,
    pub vm_id: Option<String>,
    pub status: String,
    pub result: String,
    pub duration_ms: u64,
    pub started_at: String,
    pub completed_at: String,
    pub snapshot_id: Option<String>,
    pub error: Option<String>,
}

impl ApiClient {
    pub async fn chat(
        &self,
        role: &str,
        user_message: &str,
        model: Option<&str>,
    ) -> Result<ChatResponse> {
        let url = format!("{}/chat", self.base);
        let req = ChatRequest {
            role: role.to_string(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: user_message.into(),
            }],
            model: model.map(|s| s.to_string()),
        };
        let res = self.http.post(url).json(&req).send().await?;
        let body = res.error_for_status()?.json::<ChatResponse>().await?;
        Ok(body)
    }

    pub async fn summarize_document(
        &self,
        document: &Document,
        role: Option<&str>,
    ) -> Result<SummarizeResponse> {
        let url = format!("{}/documents/summarize", self.base);
        let req = SummarizeRequest {
            document: document.clone(),
            role: role.map(|r| r.to_string()),
        };
        let res = self.http.post(url).json(&req).send().await?;
        let body = res.error_for_status()?.json::<SummarizeResponse>().await?;
        Ok(body)
    }

    pub async fn get_thesaurus(&self, role_name: &str) -> Result<ThesaurusResponse> {
        let url = format!("{}/thesaurus/{}", self.base, urlencoding::encode(role_name));
        let res = self.http.get(url).send().await?;
        let body = res.error_for_status()?.json::<ThesaurusResponse>().await?;
        Ok(body)
    }

    pub async fn get_autocomplete(
        &self,
        role_name: &str,
        query: &str,
    ) -> Result<AutocompleteResponse> {
        let url = format!(
            "{}/autocomplete/{}/{}",
            self.base,
            urlencoding::encode(role_name),
            urlencoding::encode(query)
        );
        let res = self.http.get(url).send().await?;
        let body = res
            .error_for_status()?
            .json::<AutocompleteResponse>()
            .await?;
        Ok(body)
    }

    pub async fn async_summarize_document(
        &self,
        document: &Document,
        role: Option<&str>,
    ) -> Result<AsyncSummarizeResponse> {
        let url = format!("{}/documents/async_summarize", self.base);
        let req = SummarizeRequest {
            document: document.clone(),
            role: role.map(|r| r.to_string()),
        };
        let res = self.http.post(url).json(&req).send().await?;
        let body = res
            .error_for_status()?
            .json::<AsyncSummarizeResponse>()
            .await?;
        Ok(body)
    }

    pub async fn get_task_status(&self, task_id: &str) -> Result<TaskStatusResponse> {
        let url = format!(
            "{}/summarization/task/{}/status",
            self.base,
            urlencoding::encode(task_id)
        );
        let res = self.http.get(url).send().await?;
        let body = res.error_for_status()?.json::<TaskStatusResponse>().await?;
        Ok(body)
    }

    pub async fn cancel_task(&self, task_id: &str) -> Result<TaskStatusResponse> {
        let url = format!(
            "{}/summarization/task/{}/cancel",
            self.base,
            urlencoding::encode(task_id)
        );
        let res = self.http.post(url).send().await?;
        let body = res.error_for_status()?.json::<TaskStatusResponse>().await?;
        Ok(body)
    }

    pub async fn get_queue_stats(&self) -> Result<QueueStatsResponse> {
        let url = format!("{}/summarization/queue/stats", self.base);
        let res = self.http.get(url).send().await?;
        let body = res.error_for_status()?.json::<QueueStatsResponse>().await?;
        Ok(body)
    }

    pub async fn batch_summarize_documents(
        &self,
        documents: &[Document],
        role: Option<&str>,
    ) -> Result<BatchSummarizeResponse> {
        let url = format!("{}/summarization/batch", self.base);
        let req = BatchSummarizeRequest {
            documents: documents.to_vec(),
            role: role.map(|r| r.to_string()),
        };
        let res = self.http.post(url).json(&req).send().await?;
        let body = res
            .error_for_status()?
            .json::<BatchSummarizeResponse>()
            .await?;
        Ok(body)
    }

    // VM Management APIs

    pub async fn list_vms(&self) -> Result<VmPoolListResponse> {
        let url = format!("{}/api/vm-pool", self.base);
        let res = self.http.get(url).send().await?;
        let body = res.error_for_status()?.json::<VmPoolListResponse>().await?;
        Ok(body)
    }

    pub async fn get_vm_pool_stats(&self) -> Result<VmPoolStatsResponse> {
        let url = format!("{}/api/vm-pool/stats", self.base);
        let res = self.http.get(url).send().await?;
        let body = res
            .error_for_status()?
            .json::<VmPoolStatsResponse>()
            .await?;
        Ok(body)
    }

    pub async fn get_vm_status(&self, vm_id: &str) -> Result<VmStatusResponse> {
        let url = format!("{}/api/vms/{}", self.base, urlencoding::encode(vm_id));
        let res = self.http.get(url).send().await?;
        let body = res.error_for_status()?.json::<VmStatusResponse>().await?;
        Ok(body)
    }

    pub async fn execute_vm_code(
        &self,
        code: &str,
        language: &str,
        vm_id: Option<&str>,
    ) -> Result<VmExecuteResponse> {
        let url = format!("{}/api/llm/execute", self.base);
        let req = VmExecuteRequest {
            code: code.to_string(),
            language: language.to_string(),
            agent_id: "tui-user".to_string(),
            vm_id: vm_id.map(|s| s.to_string()),
            timeout_ms: Some(30000), // 30 second default timeout
        };
        let res = self.http.post(url).json(&req).send().await?;
        let body = res.error_for_status()?.json::<VmExecuteResponse>().await?;
        Ok(body)
    }

    pub async fn list_vm_tasks(&self, vm_id: &str) -> Result<VmTasksResponse> {
        let url = format!("{}/api/vms/{}/tasks", self.base, urlencoding::encode(vm_id));
        let res = self.http.get(url).send().await?;
        let body = res.error_for_status()?.json::<VmTasksResponse>().await?;
        Ok(body)
    }

    pub async fn allocate_vm_ip(&self, vm_id: &str) -> Result<VmAllocateResponse> {
        let url = format!("{}/api/vm-pool/allocate", self.base);
        let req = VmAllocateRequest {
            vm_id: vm_id.to_string(),
        };
        let res = self.http.post(url).json(&req).send().await?;
        let body = res.error_for_status()?.json::<VmAllocateResponse>().await?;
        Ok(body)
    }

    pub async fn release_vm_ip(&self, vm_id: &str) -> Result<()> {
        let url = format!(
            "{}/api/vm-pool/release/{}",
            self.base,
            urlencoding::encode(vm_id)
        );
        let res = self.http.post(url).send().await?;
        res.error_for_status()?;
        Ok(())
    }

    pub async fn get_vm_metrics(&self, vm_id: &str) -> Result<VmMetricsResponse> {
        let url = format!(
            "{}/api/vms/{}/metrics",
            self.base,
            urlencoding::encode(vm_id)
        );
        let res = self.http.get(url).send().await?;
        let body = res.error_for_status()?.json::<VmMetricsResponse>().await?;
        Ok(body)
    }

    pub async fn get_all_vm_metrics(&self) -> Result<Vec<VmMetricsResponse>> {
        let url = format!("{}/api/vms/metrics", self.base);
        let res = self.http.get(url).send().await?;
        let body = res
            .error_for_status()?
            .json::<Vec<VmMetricsResponse>>()
            .await?;
        Ok(body)
    }

    pub async fn execute_agent_task(
        &self,
        agent_id: &str,
        task: &str,
        vm_id: Option<&str>,
    ) -> Result<VmAgentResponse> {
        let url = format!("{}/api/vm/agent/execute", self.base);
        let req = VmAgentRequest {
            agent_id: agent_id.to_string(),
            task: task.to_string(),
            vm_id: vm_id.map(|s| s.to_string()),
            timeout_ms: Some(60000), // 60 second default timeout for agent tasks
        };
        let res = self.http.post(url).json(&req).send().await?;
        let body = res.error_for_status()?.json::<VmAgentResponse>().await?;
        Ok(body)
    }
}
