//! Scoped application operations over the existing learning/correction APIs.
use super::{AgentStore, validate_id, validate_text};
use crate::learnings::{
    self, CapturedLearning, CorrectionEvent, CorrectionType, CorrectionTypeFilter,
    LearningCaptureConfig,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Parsed bytes actually saved in a learning Markdown record.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "record", rename_all = "snake_case")]
pub enum LearningRecord {
    /// Recorded failure; no command was executed by capture.
    Failure(CapturedLearning),
    /// Explicit correction; no hook was activated.
    Correction(CorrectionEvent),
}
impl LearningRecord {
    fn id(&self) -> &str {
        match self {
            Self::Failure(record) => &record.id,
            Self::Correction(record) => &record.id,
        }
    }
}
/// A stored record and its effective location.
#[derive(Debug, Clone, Serialize)]
pub struct SavedLearning {
    /// Existing-format Markdown file inside the selected data root.
    pub path: PathBuf,
    /// Parsed persisted record.
    pub record: LearningRecord,
}
/// A fresh compiled mapping artefact, distinct from hook activation.
#[derive(Debug, Serialize)]
pub struct CompiledCorrections {
    /// Generated JSON path inside a fresh export directory.
    pub path: PathBuf,
    /// Actual number of compiled mappings.
    pub mappings: usize,
    /// Actual decoded Terraphim thesaurus.
    pub thesaurus: terraphim_types::Thesaurus,
}
/// A fresh immutable KG export and the notes read back from it.
#[derive(Debug, Serialize)]
pub struct KgExport {
    /// Unique generation directory inside the explicit data root.
    pub directory: PathBuf,
    /// Actual saved Markdown notes and their paths.
    pub files: Vec<KgNote>,
}
/// One generated Logseq-compatible concept note.
#[derive(Debug, Serialize)]
pub struct KgNote {
    /// Actual saved path.
    pub path: PathBuf,
    /// Actual saved UTF-8 Markdown including `synonyms::`.
    pub markdown: String,
}

impl AgentStore {
    fn learning_relative(name: &str) -> PathBuf {
        Path::new("terraphim/learnings").join(name)
    }

    fn learning_records(&self) -> Result<Vec<SavedLearning>> {
        let directory = self.checked(Path::new("terraphim/learnings"), false)?;
        if !directory.exists() {
            return Ok(Vec::new());
        }
        ensure!(directory.is_dir(), "Learning store is not a directory");
        let mut records = Vec::new();
        let mut total = 0usize;
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_str().context("Learning filename is not UTF-8")?;
            if !name.ends_with(".md") {
                continue;
            }
            ensure!(
                records.len() < 1_000,
                "Learning store exceeds 1,000 records"
            );
            let relative = Self::learning_relative(name);
            let bytes = self
                .read(&relative)?
                .context("Learning record disappeared")?;
            total = total
                .checked_add(bytes.len())
                .context("Learning store size overflow")?;
            ensure!(
                bytes.len() <= 1_048_576 && total <= 16 * 1024 * 1024,
                "Learning store exceeds record or total byte limit"
            );
            let text = std::str::from_utf8(&bytes).context("Learning record is not UTF-8")?;
            let record = if name.starts_with("correction-") {
                LearningRecord::Correction(
                    CorrectionEvent::from_markdown(text).context("Corrupt correction record")?,
                )
            } else {
                ensure!(
                    name.starts_with("learning-"),
                    "Unknown Markdown record in learning store"
                );
                LearningRecord::Failure(
                    CapturedLearning::from_markdown(text).context("Corrupt learning record")?,
                )
            };
            validate_id(record.id())?;
            let prefix = match record {
                LearningRecord::Failure(_) => "learning",
                LearningRecord::Correction(_) => "correction",
            };
            ensure!(
                name == format!("{prefix}-{}.md", record.id()),
                "Learning filename/ID mismatch"
            );
            records.push(SavedLearning {
                path: self.checked(&relative, false)?,
                record,
            });
        }
        records.sort_by(|a, b| a.record.id().cmp(b.record.id()));
        Ok(records)
    }

    fn stage_learnings(&self) -> Result<tempfile::TempDir> {
        let stage_parent = self.checked(Path::new("terraphim/staging/placeholder"), true)?;
        let stage = tempfile::tempdir_in(stage_parent.parent().context("No staging parent")?)?;
        for record in self.learning_records()? {
            let name = record
                .path
                .file_name()
                .context("Learning record has no name")?;
            let relative = Path::new("terraphim/learnings").join(name);
            let bytes = self
                .read(&relative)?
                .context("Learning record disappeared")?;
            fs::write(stage.path().join(name), bytes)?;
        }
        Ok(stage)
    }

    fn stage_config(stage: &Path) -> LearningCaptureConfig {
        // Both with and without the environment override, select only this
        // explicit staging directory. The nonexistent project parent cannot win.
        LearningCaptureConfig::new(stage.join("unselected-project/learnings"), stage.to_owned())
    }

    fn commit_staged_learning(&self, path: &Path, replace: bool) -> Result<SavedLearning> {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .context("Invalid generated record filename")?;
        let relative = Self::learning_relative(name);
        let destination = self.checked(&relative, true)?;
        ensure!(
            replace || !destination.exists(),
            "Refusing to replace an existing learning ID"
        );
        let bytes = fs::read(path)?;
        ensure!(bytes.len() <= 1_048_576, "Learning record exceeds 1 MiB");
        let existing = self.learning_records()?;
        ensure!(
            replace || existing.len() < 1_000,
            "Learning store exceeds 1,000 records"
        );
        let mut total = bytes.len() as u64;
        for record in existing {
            if record.path != destination {
                total = total
                    .checked_add(fs::metadata(record.path)?.len())
                    .context("Learning store size overflow")?;
            }
        }
        ensure!(total <= 16 * 1024 * 1024, "Learning store exceeds 16 MiB");
        let text = std::str::from_utf8(&bytes)?;
        let record = if name.starts_with("correction-") {
            LearningRecord::Correction(
                CorrectionEvent::from_markdown(text)
                    .context("Generated correction cannot be decoded")?,
            )
        } else {
            LearningRecord::Failure(
                CapturedLearning::from_markdown(text)
                    .context("Generated failure cannot be decoded")?,
            )
        };
        validate_id(record.id())?;
        self.write_atomic(&relative, &bytes)?;
        self.learning_records()?
            .into_iter()
            .find(|saved| saved.path == destination)
            .context("Saved learning was not readable")
    }

    /// Record a supplied failed-command observation; does not execute it.
    ///
    /// # Errors
    /// Rejects blank/oversized observations, success exit codes, busy or corrupt storage.
    pub fn learning_capture(
        &self,
        command: &str,
        error: &str,
        exit_code: i32,
    ) -> Result<SavedLearning> {
        validate_text("Command", command, 65_536)?;
        validate_text("Error output", error, 524_288)?;
        ensure!(
            exit_code != 0,
            "Learning capture requires a non-zero exit code"
        );
        let _lock =
            super::lock_learning_directory(&self.checked(Path::new("terraphim/learnings"), true)?)?;
        let stage = self.stage_learnings()?;
        let path = learnings::capture_failed_command(
            &learnings::redact_secrets(command),
            error,
            exit_code,
            &Self::stage_config(stage.path()),
        )?;
        let saved = CapturedLearning::from_markdown(&fs::read_to_string(&path)?)
            .context("Generated failure cannot be decoded")?;
        ensure!(
            saved.command == learnings::redact_secrets(command)
                && saved.error_output == learnings::redact_secrets(error)
                && saved.exit_code == exit_code,
            "Existing learning Markdown cannot preserve this observation exactly; supply one failing command and representable error text"
        );
        self.commit_staged_learning(&path, false)
    }

    /// Query existing learning/correction records using the existing text query API.
    ///
    /// # Errors
    /// Rejects blank/oversized queries, invalid limits and corrupt or unreadable records.
    pub fn learning_query(&self, query: &str, limit: usize) -> Result<Vec<SavedLearning>> {
        validate_text("Query", query, 4_096)?;
        ensure!(
            (1..=100).contains(&limit),
            "Limit must be between 1 and 100"
        );
        let records = self.learning_records()?;
        let directory = self.checked(Path::new("terraphim/learnings"), false)?;
        let matches = learnings::query_all_entries(&directory, query, false)?;
        let ids: std::collections::HashSet<_> = matches.iter().map(|entry| entry.id()).collect();
        Ok(records
            .into_iter()
            .filter(|entry| ids.contains(entry.record.id()))
            .take(limit)
            .collect())
    }

    /// Annotate one exact failure ID with a correction; this adds no compiled rule.
    ///
    /// # Errors
    /// Rejects invalid/unknown IDs, invalid correction text and storage errors.
    pub fn learning_correct(&self, id: &str, correction: &str) -> Result<SavedLearning> {
        validate_id(id)?;
        validate_text("Correction", correction, 65_536)?;
        let _lock =
            super::lock_learning_directory(&self.checked(Path::new("terraphim/learnings"), true)?)?;
        let selected = self
            .learning_records()?
            .into_iter()
            .find(
                |entry| matches!(&entry.record, LearningRecord::Failure(record) if record.id == id),
            )
            .context("Exact learning ID not found")?;
        let parent = self.checked(Path::new("terraphim/staging/placeholder"), true)?;
        let stage = tempfile::tempdir_in(parent.parent().context("No staging parent")?)?;
        fs::copy(
            &selected.path,
            stage
                .path()
                .join(selected.path.file_name().context("Missing record name")?),
        )?;
        let path = learnings::correct_learning(
            &stage.path().to_owned(),
            id,
            &learnings::redact_secrets(correction),
        )?;
        let saved = CapturedLearning::from_markdown(&fs::read_to_string(&path)?)
            .context("Generated annotation cannot be decoded")?;
        ensure!(
            saved.id == id
                && saved.correction.as_deref()
                    == Some(learnings::redact_secrets(correction).as_str()),
            "Existing learning Markdown cannot preserve this annotation exactly"
        );
        self.commit_staged_learning(&path, true)
    }

    /// Add an explicit ToolPreference correction; compilation is a separate operation.
    ///
    /// # Errors
    /// Rejects blank/oversized text and storage errors. Context may be empty.
    pub fn correction_add(
        &self,
        original: &str,
        corrected: &str,
        context: &str,
    ) -> Result<SavedLearning> {
        validate_text("Original", original, 65_536)?;
        validate_text("Corrected", corrected, 65_536)?;
        ensure!(
            context.len() <= 65_536,
            "Context exceeds 65,536 UTF-8 bytes"
        );
        let _lock =
            super::lock_learning_directory(&self.checked(Path::new("terraphim/learnings"), true)?)?;
        let stage = self.stage_learnings()?;
        let path = learnings::capture_correction(
            CorrectionType::ToolPreference,
            original,
            corrected,
            context,
            &Self::stage_config(stage.path()),
        )?;
        let saved = CorrectionEvent::from_markdown(&fs::read_to_string(&path)?)
            .context("Generated correction cannot be decoded")?;
        ensure!(
            saved.original == learnings::redact_secrets(original)
                && saved.corrected == learnings::redact_secrets(corrected)
                && saved.context_description == learnings::redact_secrets(context),
            "Existing correction Markdown cannot preserve these fields exactly"
        );
        self.commit_staged_learning(&path, false)
    }

    fn new_export(&self) -> Result<PathBuf> {
        let relative = Path::new("terraphim/exports").join(uuid::Uuid::new_v4().to_string());
        let path = self.checked(&relative, true)?;
        fs::create_dir(&path)?;
        Ok(path)
    }

    /// Compile explicit corrections into a fresh JSON artefact, without activating it.
    ///
    /// # Errors
    /// Returns storage, decoding and compilation errors; never fabricates a count.
    pub fn correction_compile(&self) -> Result<CompiledCorrections> {
        let _lock =
            super::lock_learning_directory(&self.checked(Path::new("terraphim/learnings"), true)?)?;
        let stage = self.stage_learnings()?;
        let thesaurus = learnings::compile_corrections_to_thesaurus(stage.path())?;
        let output = self.new_export()?.join("compiled-corrections.json");
        let relative = output.strip_prefix(&self.root)?;
        self.write_atomic(relative, &serde_json::to_vec_pretty(&thesaurus)?)?;
        let saved = self.read(relative)?.context("Compiled output missing")?;
        let thesaurus: terraphim_types::Thesaurus = serde_json::from_slice(&saved)?;
        Ok(CompiledCorrections {
            path: output,
            mappings: thesaurus.len(),
            thesaurus,
        })
    }

    /// Export explicit corrections as fresh `synonyms::` Markdown concept notes.
    ///
    /// # Errors
    /// Refuses originals that cannot be represented by the existing comma-separated
    /// synonym syntax and any read, write or export failure. Does not register a role.
    pub fn correction_export_kg(&self) -> Result<KgExport> {
        let _lock =
            super::lock_learning_directory(&self.checked(Path::new("terraphim/learnings"), true)?)?;
        for record in self.learning_records()? {
            if let LearningRecord::Correction(record) = record.record {
                ensure!(
                    !record.original.contains([',', '\n', '\r'])
                        && !record.corrected.contains(['\n', '\r']),
                    "This correction cannot be represented by single-line comma-separated synonyms:: syntax; compiled JSON remains supported"
                );
            }
        }
        let stage = self.stage_learnings()?;
        let generated = stage.path().join("export");
        let count = learnings::export_corrections_as_kg(
            stage.path(),
            &generated,
            CorrectionTypeFilter::ToolPreference,
        )?;
        let directory = self.new_export()?;
        let mut files = Vec::new();
        if generated.exists() {
            for entry in fs::read_dir(generated)? {
                let entry = entry?;
                let output = directory.join(entry.file_name());
                let relative = output.strip_prefix(&self.root)?;
                self.write_atomic(relative, &fs::read(entry.path())?)?;
                let markdown =
                    String::from_utf8(self.read(relative)?.context("Exported note missing")?)?;
                ensure!(
                    markdown.contains("synonyms::"),
                    "Exported note lacks synonym syntax"
                );
                files.push(KgNote {
                    path: output,
                    markdown,
                });
            }
        }
        ensure!(
            files.len() == count,
            "Export count differs from saved notes"
        );
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(KgExport { directory, files })
    }
}
