//! Learning injector for cross-agent knowledge sharing
//!
//! Polls the shared markdown knowledge graph, filters learnings by trust level
//! and context relevance, and injects them into the local store.

use std::path::PathBuf;

use anyhow::Result;
use thiserror::Error;
use tracing::{debug, info};

use crate::shared_learning::types::{SharedLearning, TrustLevel};

#[derive(Error, Debug)]
pub enum InjectionError {
    #[error("shared learning store not found at {0}")]
    StoreNotFound(PathBuf),

    #[error("failed to parse learning from markdown: {0}")]
    ParseError(String),

    #[error("learning already exists in local store: {0}")]
    AlreadyExists(String),

    #[error("trust level too low: got {got:?}, need {need:?}")]
    TrustLevelTooLow { got: TrustLevel, need: TrustLevel },

    #[error("context mismatch (BM25 score below threshold)")]
    ContextMismatch,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Configuration for the learning injector
#[derive(Debug, Clone)]
pub struct InjectorConfig {
    /// Minimum trust level for auto-injection
    pub min_trust_level: TrustLevel,
    /// BM25 similarity threshold for context matching
    pub similarity_threshold: f64,
    /// Poll interval in seconds (0 = run once)
    pub poll_interval_secs: u64,
    /// Shared learnings directory to poll from
    pub shared_dir: PathBuf,
    /// Own agent ID (to skip own learnings)
    pub self_agent_id: String,
    /// Working directory for context matching
    pub working_dir: Option<PathBuf>,
}

impl Default for InjectorConfig {
    fn default() -> Self {
        let shared_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("terraphim")
            .join("learnings")
            .join("shared");

        Self {
            min_trust_level: TrustLevel::L2,
            similarity_threshold: 0.3,
            poll_interval_secs: 0,
            shared_dir,
            self_agent_id: "unknown".to_string(),
            working_dir: std::env::current_dir().ok(),
        }
    }
}

impl InjectorConfig {
    /// Set the minimum trust level for auto-injection
    pub fn with_min_trust_level(mut self, level: TrustLevel) -> Self {
        self.min_trust_level = level;
        self
    }

    /// Set the similarity threshold for BM25 context matching
    pub fn with_similarity_threshold(mut self, threshold: f64) -> Self {
        self.similarity_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Set the agent ID (to skip own learnings)
    pub fn with_self_agent_id(mut self, agent_id: String) -> Self {
        self.self_agent_id = agent_id;
        self
    }

    /// Set the working directory for context matching
    pub fn with_working_dir(mut self, dir: PathBuf) -> Self {
        self.working_dir = Some(dir);
        self
    }
}

/// Result of an injection poll
#[derive(Debug, Clone)]
pub struct InjectionResult {
    /// Number of learnings considered
    pub considered: usize,
    /// Number of learnings injected
    pub injected: usize,
    /// Number skipped due to trust level
    pub skipped_trust: usize,
    /// Number skipped because the learning was rejected
    pub skipped_rejected: usize,
    /// Number skipped due to context mismatch
    pub skipped_context: usize,
    /// Number skipped because already exists
    pub skipped_exists: usize,
    /// IDs of injected learnings
    pub injected_ids: Vec<String>,
}

impl Default for InjectionResult {
    fn default() -> Self {
        Self::new()
    }
}

impl InjectionResult {
    pub fn new() -> Self {
        Self {
            considered: 0,
            injected: 0,
            skipped_rejected: 0,
            skipped_trust: 0,
            skipped_context: 0,
            skipped_exists: 0,
            injected_ids: Vec::new(),
        }
    }

    pub fn merge(&mut self, other: InjectionResult) {
        self.considered += other.considered;
        self.injected += other.injected;
        self.skipped_rejected += other.skipped_rejected;
        self.skipped_trust += other.skipped_trust;
        self.skipped_context += other.skipped_context;
        self.skipped_exists += other.skipped_exists;
        self.injected_ids.extend(other.injected_ids);
    }
}

/// Learning injector that polls shared learnings and injects relevant ones
#[derive(Debug)]
pub struct LearningInjector {
    config: InjectorConfig,
}

impl LearningInjector {
    pub fn new(config: InjectorConfig) -> Self {
        Self { config }
    }

    /// Run injection poll (reports what would be injected)
    pub async fn run_injection(&self) -> Result<InjectionResult, InjectionError> {
        let mut result = InjectionResult::new();

        if !self.config.shared_dir.exists() {
            return Ok(result);
        }

        let shared_learnings = self.load_shared_learnings().await?;
        result.considered = shared_learnings.len();

        for learning in shared_learnings {
            if learning.source_agent == self.config.self_agent_id {
                debug!("Skipping own learning: {}", learning.id);
                continue;
            }

            // A rejected learning is never injected, whatever it claims to be
            // worth (terraphim-clients#388).
            if !crate::shared_learning::types::status_is_usable(learning.suggestion_status) {
                result.skipped_rejected += 1;
                debug!(
                    "Skipping {} (status {:?})",
                    learning.id, learning.suggestion_status
                );
                continue;
            }

            // The declared trust level in the file is advisory: the effective
            // trust follows the status and the recorded evidence.
            let effective_trust = crate::shared_learning::types::effective_trust(
                learning.suggestion_status,
                Some(&learning.quality),
            );
            if effective_trust.weight() < self.config.min_trust_level.weight() {
                result.skipped_trust += 1;
                debug!(
                    "Skipping {} (effective trust level {:?} below {:?})",
                    learning.id, effective_trust, self.config.min_trust_level
                );
                continue;
            }

            if let Some(ref working_dir) = self.config.working_dir
                && !self.should_inject(&learning, working_dir)
            {
                result.skipped_context += 1;
                debug!("Skipping {} (context mismatch)", learning.id);
                continue;
            }

            result.injected += 1;
            result.injected_ids.push(learning.id.clone());
            info!("Would inject learning: {}", learning.id);
        }

        Ok(result)
    }

    /// Load all learnings from the shared directory
    async fn load_shared_learnings(&self) -> Result<Vec<SharedLearning>, InjectionError> {
        use std::fs;

        let mut learnings = Vec::new();

        if !self.config.shared_dir.exists() {
            return Ok(learnings);
        }

        for entry in fs::read_dir(&self.config.shared_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().is_some_and(|ext| ext == "md") {
                match self.parse_learning_from_file(&path).await {
                    Ok(learning) => learnings.push(learning),
                    Err(e) => debug!("Failed to parse {}: {}", path.display(), e),
                }
            }
        }

        Ok(learnings)
    }

    /// Parse a learning from a markdown file
    async fn parse_learning_from_file(
        &self,
        path: &std::path::Path,
    ) -> Result<SharedLearning, InjectionError> {
        use chrono::Utc;

        let content = std::fs::read_to_string(path)?;
        let parts: Vec<&str> = content.splitn(3, "---").collect();

        if parts.len() < 3 {
            return Err(InjectionError::ParseError(
                "Missing YAML frontmatter".to_string(),
            ));
        }

        let yaml_content = parts[1].trim();
        let body = parts[2].trim();

        #[derive(serde::Deserialize)]
        struct Frontmatter {
            id: String,
            title: String,
            agent_id: String,
            /// Declared trust level. Advisory only: a shared file is written by
            /// another agent, so it cannot promote itself. The effective trust
            /// comes from the status and the recorded evidence.
            trust_level: String,
            /// The review status, when the file carries one.
            #[serde(default)]
            suggestion_status: Option<String>,
            /// The evidence behind a peer-validation claim.
            #[serde(default)]
            quality: Option<crate::shared_learning::types::QualityMetrics>,
            source: String,
        }

        let frontmatter: Frontmatter = serde_yaml::from_str(yaml_content)
            .map_err(|e| InjectionError::ParseError(format!("Invalid YAML frontmatter: {}", e)))?;

        let trust_level = frontmatter.trust_level.parse().unwrap_or(TrustLevel::L1);
        let suggestion_status = frontmatter
            .suggestion_status
            .as_deref()
            .and_then(|raw| raw.parse().ok())
            .unwrap_or(crate::shared_learning::types::SuggestionStatus::Pending);

        let source = match frontmatter.source.as_str() {
            "BashHook" => crate::shared_learning::types::LearningSource::BashHook,
            "AutoExtract" => crate::shared_learning::types::LearningSource::AutoExtract,
            "ToolHealth" => crate::shared_learning::types::LearningSource::ToolHealth,
            "GiteaComment" => crate::shared_learning::types::LearningSource::GiteaComment,
            "CjeVerdict" => crate::shared_learning::types::LearningSource::CjeVerdict,
            _ => crate::shared_learning::types::LearningSource::Manual,
        };

        let mut learning = SharedLearning::new(
            frontmatter.title,
            body.to_string(),
            source,
            frontmatter.agent_id,
        );

        learning.id = frontmatter.id;
        learning.trust_level = trust_level;
        learning.suggestion_status = suggestion_status;
        if let Some(quality) = frontmatter.quality {
            learning.quality = quality;
        }
        learning.created_at = Utc::now();
        learning.updated_at = Utc::now();

        Ok(learning)
    }

    /// Check if a learning should be injected based on context relevance
    fn should_inject(&self, learning: &SharedLearning, working_dir: &std::path::Path) -> bool {
        let working_dir_str = working_dir.to_string_lossy().to_lowercase();

        let dir_components: std::collections::HashSet<&str> = working_dir_str
            .split(['/', '\\', '_', '-', '.'])
            .filter(|s| !s.is_empty())
            .collect();

        let search_text = learning.extract_searchable_text().to_lowercase();
        let search_terms: std::collections::HashSet<&str> =
            search_text.split_whitespace().collect();

        let overlap: usize = dir_components.intersection(&search_terms).count();

        overlap > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rejected learning is never injected, whatever it claims to be worth
    /// (terraphim-clients#388).
    #[tokio::test]
    async fn rejected_learnings_are_never_injected() {
        let tmp = tempfile::tempdir().unwrap();
        let shared = tmp.path().join("shared");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(
            shared.join("peer-learning.md"),
            "---\nid: rejected-1\ntitle: npm install\nagent_id: peer\ntrust_level: L3\nsource: BashHook\nsuggestion_status: rejected\n---\n\nuse bun install\n",
        )
        .unwrap();

        let mut config = InjectorConfig::default().with_min_trust_level(TrustLevel::L1);
        config.shared_dir = shared;
        config.self_agent_id = "me".to_string();
        config.working_dir = Some(tmp.path().join("npm-install"));
        let injector = LearningInjector::new(config);

        let result = injector.run_injection().await.unwrap();

        assert_eq!(
            result.injected, 0,
            "a rejected learning must not be injected"
        );
        assert_eq!(result.skipped_rejected, 1);
    }

    /// A learning that only *claims* L3 without an approval stays L1, and a
    /// peer-validated one (with evidence) is injected (terraphim-clients#388).
    #[tokio::test]
    async fn declared_trust_without_evidence_does_not_promote_a_learning() {
        let tmp = tempfile::tempdir().unwrap();
        let shared = tmp.path().join("shared");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(
            shared.join("claimed.md"),
            "---\nid: claimed-1\ntitle: npm install\nagent_id: peer\ntrust_level: L3\nsource: BashHook\n---\n\nuse bun install\n",
        )
        .unwrap();

        let mut config = InjectorConfig::default().with_min_trust_level(TrustLevel::L2);
        config.shared_dir = shared.clone();
        config.self_agent_id = "me".to_string();
        config.working_dir = Some(tmp.path().join("npm-install"));
        let injector = LearningInjector::new(config);

        let result = injector.run_injection().await.unwrap();
        assert_eq!(
            result.injected, 0,
            "a file that claims L3 without an approval must not be injected"
        );
        assert_eq!(result.skipped_trust, 1);

        // With the evidence behind the claim, the same learning is injected.
        std::fs::write(
            shared.join("claimed.md"),
            "---\nid: claimed-1\ntitle: npm install\nagent_id: peer\ntrust_level: L2\nsource: BashHook\nquality:\n  applied_count: 3\n  effective_count: 3\n  agent_count: 2\n  agent_names:\n    - alpha\n    - beta\n  last_applied_at: null\n  success_rate: 1.0\n---\n\nuse bun install\n",
        )
        .unwrap();

        let result = injector.run_injection().await.unwrap();
        assert_eq!(
            result.injected, 1,
            "a peer-validated learning must still be injected"
        );
    }

    #[test]
    fn test_injector_config_defaults() {
        let config = InjectorConfig::default();
        assert_eq!(config.min_trust_level, TrustLevel::L2);
        assert_eq!(config.similarity_threshold, 0.3);
        assert_eq!(config.poll_interval_secs, 0);
    }

    #[test]
    fn test_injection_result_merge() {
        let mut result1 = InjectionResult::new();
        result1.considered = 10;
        result1.injected = 3;
        result1.skipped_trust = 5;
        result1.skipped_context = 1;
        result1.skipped_exists = 1;

        let result2 = InjectionResult::new();

        result1.merge(result2);

        assert_eq!(result1.considered, 10);
        assert_eq!(result1.injected, 3);
    }

    #[test]
    fn test_should_inject_keyword_match() {
        let config = InjectorConfig::default();
        let injector = LearningInjector::new(config);

        let learning = SharedLearning::new(
            "Rust Error Handling".to_string(),
            "Use Result<T, E> for error handling in Rust".to_string(),
            crate::shared_learning::types::LearningSource::BashHook,
            "other-agent".to_string(),
        );

        let working_dir = PathBuf::from("/home/user/projects/terraphim-rust");

        assert!(injector.should_inject(&learning, &working_dir));
    }

    #[test]
    fn test_should_inject_no_match() {
        let config = InjectorConfig::default();
        let injector = LearningInjector::new(config);

        let learning = SharedLearning::new(
            "Python Django Tips".to_string(),
            "Use class-based views in Django".to_string(),
            crate::shared_learning::types::LearningSource::BashHook,
            "other-agent".to_string(),
        );

        let working_dir = PathBuf::from("/home/user/projects/terraphim-rust");

        assert!(!injector.should_inject(&learning, &working_dir));
    }
}
