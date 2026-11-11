//! `--role` must resolve against the user role configuration (#405).
//!
//! Before the fix, `terraphim-grep --role X` only looked for
//! `thesaurus-X.json` in a project `.terraphim/` directory. Roles defined in
//! `~/.config/terraphim/embedded_config.json` — and the Markdown knowledge
//! graph each role points at — were invisible, so grep and terraphim-agent
//! did not share a vocabulary.
//!
//! Each test runs the compiled binary with an isolated `HOME` /
//! `XDG_CONFIG_HOME`, so no host configuration leaks in.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn grep_binary() -> &'static str {
    env!("CARGO_BIN_EXE_terraphim-grep")
}

struct Fixture {
    _root: tempfile::TempDir,
    home: PathBuf,
    repo: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::TempDir::new().expect("tempdir");
        let home = root.path().join("home");
        let repo = root.path().join("repo");
        std::fs::create_dir_all(home.join(".config/terraphim")).expect("config dir");
        std::fs::create_dir_all(&repo).expect("repo dir");
        Self {
            _root: root,
            home,
            repo,
        }
    }

    fn config_home(&self) -> PathBuf {
        self.home.join(".config")
    }

    /// A user config with one role ("Code Reviewer", shortname "code-reviewer")
    /// whose KG is a Markdown directory.
    fn write_user_role(&self, kg_dir: &Path) {
        let config = format!(
            r#"{{
  "id": "Desktop",
  "global_shortcut": null,
  "roles": {{
    "Code Reviewer": {{
      "name": "Code Reviewer",
      "shortname": "code-reviewer",
      "relevance_function": "title-scorer",
      "terraphim_it": false,
      "theme": "default",
      "haystacks": [],
      "kg": {{
        "knowledge_graph_local": {{
          "input_type": "markdown",
          "path": "{}"
        }},
        "public": false,
        "publish": false
      }}
    }}
  }},
  "default_role": "Code Reviewer",
  "selected_role": "Code Reviewer"
}}"#,
            kg_dir.display()
        );
        std::fs::write(
            self.config_home().join("terraphim/embedded_config.json"),
            config,
        )
        .expect("write user config");
    }

    fn write_repo_file(&self) {
        std::fs::write(
            self.repo.join("service.rs"),
            "// code review notes: rollback plan for the deploy\nfn deploy() {}\n",
        )
        .expect("write repo file");
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(grep_binary())
            .current_dir(&self.repo)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.config_home())
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env_remove("TERRAPHIM_CONFIG")
            .env_remove("OPENROUTER_API_KEY")
            .env("RUST_LOG", "info")
            .args(args)
            .output()
            .expect("failed to run terraphim-grep")
    }

    fn run_json(&self, role: &str, query: &str) -> (Output, serde_json::Value) {
        let output = self.run(&[
            query,
            "--search-only",
            "--json",
            "--role",
            role,
            "--paths",
            self.repo.to_str().unwrap(),
            "-n",
            "5",
        ]);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|error| {
            panic!(
                "stdout should be JSON ({error})\nstdout: {stdout}\nstderr: {}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output, json)
    }
}

fn concept_names(json: &serde_json::Value) -> Vec<String> {
    json["concepts"]
        .as_array()
        .expect("concepts array")
        .iter()
        .filter_map(|concept| concept["name"].as_str().map(str::to_string))
        .collect()
}

fn write_code_reviewer_kg(fixture: &Fixture) -> PathBuf {
    let kg = fixture.home.join("kg");
    std::fs::create_dir_all(&kg).expect("kg dir");
    std::fs::write(
        kg.join("code review.md"),
        "# Code review\n\nsynonyms:: rollback plan, regression risk\n",
    )
    .expect("write kg concept");
    kg
}

#[test]
fn role_display_name_loads_the_user_role_thesaurus() {
    let fixture = Fixture::new();
    let kg = write_code_reviewer_kg(&fixture);
    fixture.write_user_role(&kg);
    fixture.write_repo_file();

    let (output, json) = fixture.run_json("Code Reviewer", "rollback plan");

    assert!(
        output.status.success(),
        "role search should succeed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let concepts = concept_names(&json);
    assert!(
        concepts.iter().any(|name| name.contains("rollback")),
        "expected the role's KG synonym in concepts, got {concepts:?}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        json["stats"]["kg_hits"].as_u64().unwrap_or(0) >= 1,
        "expected kg_hits >= 1, got {}",
        json["stats"]["kg_hits"]
    );
}

#[test]
fn role_shortname_loads_the_user_role_thesaurus() {
    let fixture = Fixture::new();
    let kg = write_code_reviewer_kg(&fixture);
    fixture.write_user_role(&kg);
    fixture.write_repo_file();

    let (output, json) = fixture.run_json("code-reviewer", "rollback plan");

    assert!(
        output.status.success(),
        "shortname search should succeed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        concept_names(&json)
            .iter()
            .any(|name| name.contains("rollback")),
        "shortname must resolve the same role vocabulary, got {:?}",
        concept_names(&json)
    );
}

#[test]
fn project_thesaurus_still_takes_precedence_over_the_user_role() {
    let fixture = Fixture::new();
    let kg = write_code_reviewer_kg(&fixture);
    fixture.write_user_role(&kg);
    fixture.write_repo_file();

    // A project thesaurus for the same role, with its own vocabulary. The
    // repository text uses the phrase so the query returns a chunk; exit 0 is
    // reserved for a chunk (or an answer), not a KG-only concept hit (#450).
    let project = fixture.repo.join(".terraphim");
    std::fs::create_dir_all(&project).expect("project dir");
    std::fs::write(
        project.join("thesaurus-code-reviewer.json"),
        r#"{
  "name": "project",
  "data": {
    "project-sentinel": { "id": 1, "nterm": "project sentinel" }
  }
}"#,
    )
    .expect("write project thesaurus");
    std::fs::write(
        fixture.repo.join("sentinel.txt"),
        "project sentinel is the documented marker\n",
    )
    .expect("write sentinel text");

    let (output, json) = fixture.run_json("code-reviewer", "project-sentinel");

    assert!(
        output.status.success(),
        "project thesaurus search should succeed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let concepts = concept_names(&json);
    assert!(
        concepts.iter().any(|name| name == "project-sentinel"),
        "expected the project thesaurus concept, got {concepts:?}"
    );

    // The user role's KG must not be merged in: the project file wins.
    let (_, json) = fixture.run_json("code-reviewer", "rollback plan");
    let concepts = concept_names(&json);
    assert!(
        !concepts.iter().any(|name| name.contains("rollback")),
        "the user role KG must not override a project thesaurus, got {concepts:?}"
    );
}
