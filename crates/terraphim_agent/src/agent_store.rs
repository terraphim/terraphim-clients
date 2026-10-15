//! Explicit local storage shared by the agent CLI and MCP adapter.
//!
//! Preserves the existing evolution JSON and learning Markdown formats using
//! Terraphim types. Never executes commands, installs hooks or activates rules.
use crate::memory_retrieve::collect_memory_items;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};
use terraphim_agent_evolution::{
    AgentEvolutionSystem, ImportanceLevel, LessonsState, MemoryItem, MemoryItemType, MemoryState,
};

mod learnings;
mod transaction_lock;
pub use learnings::{CompiledCorrections, KgExport, KgNote, LearningRecord, SavedLearning};
use transaction_lock::TransactionLock;

const MAX_STORE_BYTES: u64 = 128 * 1024 * 1024;

// Both the legacy CLI learning APIs and explicit MCP transactions hold this
// same lock. Staged core operations acquire an independent staging-directory
// lock; they cannot unlock or overwrite the destination transaction.
pub(crate) fn lock_learning_directory(directory: &Path) -> Result<TransactionLock> {
    let absolute = if directory.is_absolute() {
        directory.to_owned()
    } else {
        std::env::current_dir()?.join(directory)
    };
    AgentStore::open(&absolute)?.lock(Path::new(".write.lock"))
}

#[derive(Serialize, Deserialize)]
struct Snapshot {
    #[serde(default = "agent_id")]
    agent_id: String,
    #[serde(default)]
    saved_at: String,
    memory: MemoryState,
    lessons: LessonsState,
}
fn agent_id() -> String {
    "cli-agent".into()
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            agent_id: agent_id(),
            saved_at: String::new(),
            memory: MemoryState::default(),
            lessons: LessonsState::default(),
        }
    }
}

/// Explicit local data root shared with `TERRAPHIM_DEFAULT_DATA_PATH`.
#[derive(Debug, Clone)]
pub struct AgentStore {
    root: PathBuf,
}
impl AgentStore {
    /// Select an absolute data root, creating that directory when absent.
    ///
    /// # Errors
    /// Rejects relative/traversing roots, symlink roots and inaccessible paths.
    pub fn open(root: &Path) -> Result<Self> {
        ensure!(root.is_absolute(), "Agent data root must be absolute");
        ensure!(
            !root.components().any(|c| matches!(c, Component::ParentDir)),
            "Agent data root must not contain '..'"
        );
        if let Ok(meta) = fs::symlink_metadata(root) {
            ensure!(
                !meta.file_type().is_symlink(),
                "Agent data root must not be a symlink"
            );
        }
        fs::create_dir_all(root).context("Cannot create agent data root")?;
        let root = root
            .canonicalize()
            .context("Cannot resolve agent data root")?;
        ensure!(root.is_dir(), "Agent data root must be a directory");
        Ok(Self { root })
    }

    /// Use the CLI's existing environment override or OS configuration root.
    ///
    /// # Errors
    /// Returns an error for a missing default location or invalid explicit root.
    pub fn for_cli() -> Result<Self> {
        let root = std::env::var_os("TERRAPHIM_DEFAULT_DATA_PATH")
            .map(PathBuf::from)
            .or_else(dirs::config_dir)
            .context("No local agent configuration directory")?;
        Self::open(&root)
    }

    /// Effective canonical root; the working directory does not scope it.
    pub fn root(&self) -> &Path {
        &self.root
    }
    /// Existing CLI evolution-file location inside this root.
    pub fn memory_path(&self) -> PathBuf {
        self.root.join("terraphim/evolution/cli-agent.json")
    }

    fn checked(&self, relative: &Path, create_parents: bool) -> Result<PathBuf> {
        ensure!(
            self.root.canonicalize()? == self.root,
            "Agent data root changed"
        );
        ensure!(
            relative
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
            "Invalid store-relative path"
        );
        let path = self.root.join(relative);
        let mut cursor = self.root.clone();
        let components: Vec<_> = relative.components().collect();
        for (i, component) in components.iter().enumerate() {
            cursor.push(component);
            let final_component = i + 1 == components.len();
            match fs::symlink_metadata(&cursor) {
                Ok(meta) => {
                    ensure!(
                        !meta.file_type().is_symlink(),
                        "Refusing symlink in agent store: {}",
                        cursor.display()
                    );
                    if !final_component {
                        ensure!(
                            meta.is_dir(),
                            "Store parent is not a directory: {}",
                            cursor.display()
                        );
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if create_parents && !final_component {
                        match fs::create_dir(&cursor) {
                            Ok(()) => (),
                            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
                            Err(error) => return Err(error.into()),
                        }
                        let meta = fs::symlink_metadata(&cursor)?;
                        ensure!(
                            meta.is_dir() && !meta.file_type().is_symlink(),
                            "Store parent changed during creation"
                        );
                    }
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(path)
    }

    fn read(&self, relative: &Path) -> Result<Option<Vec<u8>>> {
        let path = self.checked(relative, false)?;
        let metadata = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        ensure!(
            metadata.is_file() && metadata.len() <= MAX_STORE_BYTES,
            "Agent store must be a regular file of at most 128 MiB"
        );
        let data = fs::read(&path).with_context(|| format!("Cannot read {}", path.display()))?;
        ensure!(
            data.len() as u64 <= MAX_STORE_BYTES,
            "Agent store exceeds 128 MiB"
        );
        self.checked(relative, false)?;
        Ok(Some(data))
    }

    fn memory_snapshot(&self) -> Result<Snapshot> {
        match self.read(Path::new("terraphim/evolution/cli-agent.json"))? {
            None => Ok(Snapshot::default()),
            Some(data) => {
                let snapshot: Snapshot = serde_json::from_slice(&data)
                    .context("Agent memory store is corrupt; existing bytes were preserved")?;
                ensure!(
                    snapshot.agent_id == "cli-agent",
                    "Unexpected agent identity in memory store"
                );
                Ok(snapshot)
            }
        }
    }

    fn lock(&self, relative: &Path) -> Result<TransactionLock> {
        let path = self.checked(relative, true)?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        ensure!(
            file.metadata()?.is_file(),
            "Store lock is not a regular file"
        );
        let transaction =
            TransactionLock::acquire(file).context("Agent store is busy; retry this request")?;
        self.checked(relative, false)?;
        Ok(transaction)
    }

    fn write_atomic(&self, relative: &Path, bytes: &[u8]) -> Result<()> {
        ensure!(
            bytes.len() as u64 <= MAX_STORE_BYTES,
            "Agent store exceeds 128 MiB"
        );
        let path = self.checked(relative, true)?;
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            ensure!(
                metadata.is_file(),
                "Store destination is not a regular file"
            );
        }
        let mut temporary =
            tempfile::NamedTempFile::new_in(path.parent().context("Missing store parent")?)?;
        temporary.write_all(bytes)?;
        temporary.as_file().sync_all()?;
        self.checked(relative, false)?;
        temporary
            .persist(&path)
            .map_err(|error| error.error)
            .context("Cannot commit agent store")?;
        ensure!(
            self.read(relative)?.as_deref() == Some(bytes),
            "Agent store readback differs from committed bytes"
        );
        Ok(())
    }

    /// Load the existing typed snapshot without replacing corrupt data.
    ///
    /// # Errors
    /// Returns filesystem, scope and JSON errors instead of an empty replacement.
    pub fn load_evolution(&self) -> Result<AgentEvolutionSystem> {
        let snapshot = self.memory_snapshot()?;
        let mut evolution = AgentEvolutionSystem::new(agent_id());
        evolution.memory.current_state = snapshot.memory;
        evolution.lessons.current_state = snapshot.lessons;
        Ok(evolution)
    }

    /// Capture authored UTF-8 content and read the saved item back.
    ///
    /// Existing `MemoryState` semantics retain the latest 100 short-term items.
    /// The advisory transaction lock prevents cooperating CLI/MCP writers losing
    /// acknowledged records. A busy store returns an error, never a success.
    ///
    /// # Errors
    /// Rejects blank/oversized content, oversized tags, busy/corrupt stores and I/O failures.
    pub fn memory_capture(
        &self,
        content: &str,
        provenance_tag: Option<&str>,
    ) -> Result<MemoryItem> {
        validate_text("Memory lesson", content, 1_000_000)?;
        if let Some(tag) = provenance_tag {
            validate_text("Provenance tag", tag, 512)?;
        }
        let _lock = self.lock(Path::new("terraphim/evolution/cli-agent.lock"))?;
        let mut snapshot = self.memory_snapshot()?;
        let memory = MemoryItem {
            id: uuid::Uuid::new_v4().to_string(),
            item_type: MemoryItemType::Experience,
            content: content.to_owned(),
            created_at: chrono::Utc::now(),
            last_accessed: None,
            access_count: 0,
            importance: ImportanceLevel::Medium,
            tags: provenance_tag
                .map(|tag| vec![format!("provenance:{tag}")])
                .unwrap_or_default(),
            associations: HashMap::new(),
        };
        let id = memory.id.clone();
        snapshot.memory.add_memory(memory);
        snapshot.saved_at = chrono::Utc::now().to_rfc3339();
        self.write_atomic(
            Path::new("terraphim/evolution/cli-agent.json"),
            &serde_json::to_vec_pretty(&snapshot)?,
        )?;
        self.memory_show(&id)
    }

    /// Read one exact stored ID. Prefixes and path-like IDs are not accepted.
    ///
    /// # Errors
    /// Returns an error for invalid/unknown IDs or unreadable/corrupt storage.
    pub fn memory_show(&self, id: &str) -> Result<MemoryItem> {
        validate_id(id)?;
        collect_memory_items(&self.memory_snapshot()?.memory)
            .into_iter()
            .find(|item| item.id == id)
            .with_context(|| format!("Memory ID '{id}' not found in the configured store"))
    }

    /// List stored items in stable creation-time/ID order with bounded pagination.
    ///
    /// # Errors
    /// Rejects limits outside 1..=100 and unreadable/corrupt storage.
    pub fn memory_list(&self, limit: usize, offset: usize) -> Result<Vec<MemoryItem>> {
        ensure!(
            (1..=100).contains(&limit),
            "Limit must be between 1 and 100"
        );
        let mut items = collect_memory_items(&self.memory_snapshot()?.memory);
        items.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(items.into_iter().skip(offset).take(limit).collect())
    }
}
fn validate_text(name: &str, value: &str, max: usize) -> Result<()> {
    ensure!(!value.trim().is_empty(), "{name} must not be blank");
    ensure!(value.len() <= max, "{name} exceeds {max} UTF-8 bytes");
    Ok(())
}
fn validate_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 200
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')),
        "ID must be a complete identifier, not a path or prefix"
    );
    Ok(())
}
