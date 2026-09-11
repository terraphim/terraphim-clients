//! Runtime detection of whether the current binary is managed by a system
//! package manager (e.g. pacman), as opposed to Terraphim's own self-update
//! mechanism (Gitea #247).
//!
//! Detection is deterministic and requires **both**:
//! 1. A marker file at a known path containing exactly one supported
//!    manager's name.
//! 2. The canonicalized current executable path being a path-component-wise
//!    descendant of that manager's canonical install prefix.
//!
//! Marker alone or prefix alone never claims package-managed ownership, and
//! any ambiguity or I/O error (missing/unreadable marker, a path that fails
//! to canonicalize) resolves to [`UpdatePolicy::SelfManaged`] -- detection
//! never panics and always fails safe toward preserving today's self-update
//! behavior.
//!
//! This module is plain data + pure functions: no `cfg!`, no Cargo feature.
//! [`detect_update_policy`] takes every filesystem input as a parameter, so
//! tests can exercise it against `tempfile::TempDir`-rooted stand-ins
//! without touching the real `/usr` tree or process environment.
//! [`detect_update_policy_default`] is the only function that touches real
//! process state.

use std::fs;
use std::path::Path;

/// A supported system package manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Pacman,
    // Extensible: future supported managers add a variant + a
    // `MANAGED_PREFIXES` table entry (see module docs).
}

impl PackageManager {
    /// The exact marker-file value (case-sensitive) that identifies this
    /// manager. Also used as the manager's human-readable name.
    pub fn name(&self) -> &'static str {
        match self {
            PackageManager::Pacman => "pacman",
        }
    }

    /// The operator-facing update command for this manager.
    pub fn update_command(&self) -> &'static str {
        match self {
            PackageManager::Pacman => "sudo pacman -Syu",
        }
    }

    /// Parse a trimmed marker-file value into a supported manager. Returns
    /// `None` for anything that isn't an exact match (unsupported name,
    /// wrong case, or content with embedded whitespace/newlines).
    fn from_marker_value(value: &str) -> Option<Self> {
        match value {
            "pacman" => Some(PackageManager::Pacman),
            _ => None,
        }
    }
    // NOTE: `name()` (above) doubles as `marker_value` -- the marker file's
    // accepted content is defined to be exactly the manager's display name.
}

/// Runtime update policy for a Terraphim binary: whether self-update
/// (network check/download/install) is safe, or whether updates must be
/// deferred to a system package manager instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdatePolicy {
    /// Default: self-update is safe. Resolved whenever detection is
    /// ambiguous, fails, or simply doesn't match a package-managed install.
    SelfManaged,
    /// The running binary was installed by a package manager; self-update
    /// must be a no-op refusal that guides the operator to that manager's
    /// own update command instead.
    PackageManaged {
        manager: PackageManager,
        update_command: String,
    },
}

/// Real path to the package-manager marker file. O4's packaging is
/// responsible for installing this file; this crate never writes it.
pub const MARKER_PATH: &str = "/usr/share/terraphim/package-manager";

/// Table of (manager, managed prefix) pairs used by
/// [`detect_update_policy_default`]. Only `pacman` -> `/usr/bin` is wired up
/// today; more managers can be added here later without changing the
/// detection contract's shape.
pub const MANAGED_PREFIXES: &[(PackageManager, &str)] = &[(PackageManager::Pacman, "/usr/bin")];

/// Read and validate the marker file, returning the supported manager it
/// names, or `None` if the file is missing/unreadable or its contents don't
/// exactly match one supported manager (after trimming surrounding
/// whitespace, which permits a single trailing newline).
fn read_marker(marker_path: &Path) -> Option<PackageManager> {
    let contents = fs::read_to_string(marker_path).ok()?;
    PackageManager::from_marker_value(contents.trim())
}

/// Pure detection: takes every filesystem input as a parameter. No global
/// state, no env var reads, no hardcoded paths. Safe to call with temp-dir
/// stand-ins for the executable path, marker path, and prefix table.
///
/// Never panics: any I/O error resolves to [`UpdatePolicy::SelfManaged`].
pub fn detect_update_policy(
    current_exe: &Path,
    marker_path: &Path,
    managed_prefixes: &[(PackageManager, &Path)],
) -> UpdatePolicy {
    let Some(manager) = read_marker(marker_path) else {
        return UpdatePolicy::SelfManaged;
    };

    let Ok(canonical_exe) = fs::canonicalize(current_exe) else {
        return UpdatePolicy::SelfManaged;
    };

    for (candidate_manager, prefix) in managed_prefixes {
        if *candidate_manager != manager {
            continue;
        }
        let Ok(canonical_prefix) = fs::canonicalize(prefix) else {
            continue;
        };
        // `Path::starts_with` compares whole path components, not raw
        // strings, so a sibling directory like `/usr/bin-evil` can never
        // spoof `/usr/bin` here.
        if canonical_exe.starts_with(&canonical_prefix) {
            return UpdatePolicy::PackageManaged {
                manager,
                update_command: manager.update_command().to_string(),
            };
        }
    }

    UpdatePolicy::SelfManaged
}

/// The only function that touches real process state: resolves
/// `std::env::current_exe()`, the real marker path ([`MARKER_PATH`]), and
/// the real prefix table ([`MANAGED_PREFIXES`]), then delegates to
/// [`detect_update_policy`]. Called once, at startup / `UpdaterConfig`
/// construction.
pub fn detect_update_policy_default() -> UpdatePolicy {
    let Ok(current_exe) = std::env::current_exe() else {
        return UpdatePolicy::SelfManaged;
    };
    let prefixes: Vec<(PackageManager, &Path)> = MANAGED_PREFIXES
        .iter()
        .map(|(manager, prefix)| (*manager, Path::new(*prefix)))
        .collect();
    detect_update_policy(&current_exe, Path::new(MARKER_PATH), &prefixes)
}

/// Stable operator-facing guidance for a `PackageManaged` policy. Returns an
/// empty string for `SelfManaged` (there is nothing to guide toward).
pub fn guidance(policy: &UpdatePolicy, bin_name: &str) -> String {
    match policy {
        UpdatePolicy::SelfManaged => String::new(),
        UpdatePolicy::PackageManaged { update_command, .. } => format!(
            "{bin_name} was installed via a system package manager; run `{update_command}` to update it."
        ),
    }
}
