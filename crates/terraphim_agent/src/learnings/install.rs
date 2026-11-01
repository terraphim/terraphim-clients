//! Hook installation for AI agents.
//!
//! This module provides functionality to install hooks for various AI agents
//! (Claude Code, Codex, opencode) to capture failed commands as learnings.
//!
//! # Usage
//!
//! ```rust,ignore
//! use terraphim_agent::learnings::{AgentType, install_hook};
//!
//! install_hook(AgentType::Claude).await?;
//! ```

use std::path::PathBuf;

use thiserror::Error;

/// AI agent type for hook installation.
#[derive(Debug, Clone, Copy, PartialEq, clap::ValueEnum)]
pub enum AgentType {
    /// Claude Code (Claude CLI)
    Claude,
    /// OpenAI Codex CLI
    Codex,
    /// Opencode CLI
    Opencode,
    /// Pi agent (pi_agent_rust) — installs the pi-terraphim-learn extension package
    Pi,
}

impl AgentType {
    /// Get the agent's name as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentType::Claude => "claude",
            AgentType::Codex => "codex",
            AgentType::Opencode => "opencode",
            AgentType::Pi => "pi",
        }
    }

    /// Get the agent's config directory.
    ///
    /// pi's config root is `~/.pi/agent` (pi resolves it from the *home*
    /// directory, not the XDG config directory), so Pi must not use
    /// `dirs::config_dir()` here. Refs terraphim-clients#458.
    pub fn config_dir(&self) -> Option<PathBuf> {
        match self {
            AgentType::Claude => dirs::config_dir().map(|d| d.join("claude")),
            AgentType::Codex => dirs::config_dir().map(|d| d.join("codex")),
            AgentType::Opencode => dirs::config_dir().map(|d| d.join("opencode")),
            AgentType::Pi => dirs::home_dir().map(|h| h.join(".pi").join("agent")),
        }
    }

    /// Get the hook script content for this agent.
    ///
    /// Returns `None` for agents that are not wired through a shell hook.
    ///
    /// pi (`pi_agent_rust`) loads extension packages rather than shell hooks,
    /// so it has no hook script to write — see [`pi_install_guidance`] for the
    /// real installation contract. Refs terraphim-clients#458.
    pub fn hook_script(&self) -> Option<String> {
        match self {
            AgentType::Claude => Some(
                r#"#!/bin/bash
# Terraphim Agent Hook for Claude Code
# This hook captures failed commands for learning

set -e

# Capture the tool execution result and pipe to terraphim-agent
if command -v terraphim-agent >/dev/null 2>&1; then
    # Read stdin
    INPUT=$(cat)

    # Pass through to terraphim-agent hook
    echo "$INPUT" | terraphim-agent learn hook --format claude 2>/dev/null || true

    # Always pass through original input (fail-open)
    echo "$INPUT"
else
    # terraphim-agent not installed, pass through unchanged
    cat
fi
"#
                .to_string(),
            ),
            AgentType::Codex => Some(
                r#"#!/bin/bash
# Terraphim Agent Hook for OpenAI Codex
# This hook captures failed commands for learning

set -e

# Capture the tool execution result and pipe to terraphim-agent
if command -v terraphim-agent >/dev/null 2>&1; then
    # Read stdin
    INPUT=$(cat)

    # Pass through to terraphim-agent hook
    echo "$INPUT" | terraphim-agent learn hook --format codex 2>/dev/null || true

    # Always pass through original input (fail-open)
    echo "$INPUT"
else
    # terraphim-agent not installed, pass through unchanged
    cat
fi
"#
                .to_string(),
            ),
            AgentType::Opencode => Some(
                r#"#!/bin/bash
# Terraphim Agent Hook for Opencode
# This hook captures failed commands for learning

set -e

# Capture the tool execution result and pipe to terraphim-agent
if command -v terraphim-agent >/dev/null 2>&1; then
    # Read stdin
    INPUT=$(cat)

    # Pass through to terraphim-agent hook
    echo "$INPUT" | terraphim-agent learn hook --format opencode 2>/dev/null || true

    # Always pass through original input (fail-open)
    echo "$INPUT"
else
    # terraphim-agent not installed, pass through unchanged
    cat
fi
"#
                .to_string(),
            ),
            // pi installs an extension package, not a shell hook.
            AgentType::Pi => None,
        }
    }

    /// Get the hook file path for this agent.
    ///
    /// Returns `None` for agents that are not wired through a shell hook.
    /// pi (`pi_agent_rust`) loads extension packages, so it has no hook file
    /// path at all. Refs terraphim-clients#458.
    pub fn hook_path(&self) -> Option<PathBuf> {
        match self {
            // pi consumes an extension package, not a shell hook: there is no
            // hook file to point at.
            AgentType::Pi => None,
            _ => self.config_dir().map(|d| d.join("terraphim-hook.sh")),
        }
    }
}

/// Installation guidance for pi (`pi_agent_rust`).
///
/// pi does not consume a shell hook: it loads extension packages that register
/// event handlers. The real contract is therefore the
/// `packages/pi-terraphim-learn` extension package, installed with
/// `pi install`. Refs terraphim-clients#458.
///
/// The content mirrors `packages/pi-terraphim-learn/README.md`: the
/// `terraphim-agent >= 1.21.0` requirement is stated at install time, the
/// extension-trust acknowledgement (`pi doctor`) is included, and the smoke
/// command is the exact JSON envelope from the README.
pub fn pi_install_guidance() -> &'static str {
    concat!(
        "Pi agent (pi_agent_rust) uses an extension package, not a shell hook.\n",
        "\n",
        "  1. Install the extension package (requires terraphim-agent >= 1.21.0 on PATH):\n",
        "       pi install <terraphim-clients>/packages/pi-terraphim-learn\n",
        "\n",
        "  2. Acknowledge extension trust if `pi doctor` prompts.\n",
        "\n",
        "  3. Smoke-test capture without pi:\n",
        "       echo '{\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"false\"},",
        "\"tool_result\":{\"exit_code\":1,\"stdout\":\"\",\"stderr\":\"x\"}}' \\\n",
        "         | terraphim-agent learn hook --format claude\n",
        "       ls -lt ~/.local/share/terraphim/learnings/ | head\n",
        "\n",
        "  The extension listens for pi's onToolResult event and forwards failed\n",
        "  bash-like commands to `terraphim-agent learn hook --format claude`.\n",
        "  See packages/pi-terraphim-learn/README.md for details.",
    )
}

/// Errors that can occur during hook installation.
#[derive(Debug, Error)]
pub enum InstallError {
    /// Failed to create config directory
    #[error("failed to create config directory: {0}")]
    ConfigDirError(String),
    /// Failed to write hook file
    #[error("failed to write hook file: {0}")]
    WriteError(#[from] std::io::Error),
    /// Agent config directory not found
    #[error("agent config directory not found")]
    ConfigNotFound,
    /// Hook already exists
    #[error("hook already exists at {0}")]
    AlreadyExists(String),
}

/// Install hook for the specified AI agent.
///
/// Creates a hook script in the agent's config directory that captures
/// failed commands and forwards them to terraphim-agent for learning.
///
/// Agents that are not wired through a shell hook (currently pi) print their
/// real installation contract instead of writing a hook file. Refs
/// terraphim-clients#458.
///
/// # Arguments
///
/// * `agent` - The AI agent type to install the hook for
///
/// # Returns
///
/// Ok(()) if installation succeeds, Err(InstallError) otherwise.
///
/// # Examples
///
/// ```rust,ignore
/// use terraphim_agent::learnings::{AgentType, install_hook};
///
/// install_hook(AgentType::Claude).await?;
/// ```
pub async fn install_hook(agent: AgentType) -> Result<(), InstallError> {
    // pi has no shell hook: it installs an extension package instead. Branch
    // explicitly on the agent rather than inferring it from `hook_script()`,
    // so a future agent without a shell hook cannot silently be routed to the
    // Pi guidance. Refs terraphim-clients#458.
    if matches!(agent, AgentType::Pi) {
        println!("{}", pi_install_guidance());
        return Ok(());
    }

    let hook_content = agent
        .hook_script()
        .expect("non-Pi agents always have a shell hook script");

    let config_dir = agent.config_dir().ok_or(InstallError::ConfigNotFound)?;
    let hook_path = agent.hook_path().ok_or(InstallError::ConfigNotFound)?;

    // Create config directory if it doesn't exist
    tokio::fs::create_dir_all(&config_dir)
        .await
        .map_err(|e| InstallError::ConfigDirError(e.to_string()))?;

    // Check if hook already exists
    if hook_path.exists() {
        return Err(InstallError::AlreadyExists(
            hook_path.to_string_lossy().to_string(),
        ));
    }

    // Write hook script
    tokio::fs::write(&hook_path, hook_content)
        .await
        .map_err(InstallError::WriteError)?;

    // Make hook executable (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = tokio::fs::metadata(&hook_path)
            .await
            .map_err(InstallError::WriteError)?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o755);
        tokio::fs::set_permissions(&hook_path, permissions)
            .await
            .map_err(InstallError::WriteError)?;
    }

    println!(
        "Installed Terraphim hook for {} at: {}",
        agent.as_str(),
        hook_path.display()
    );
    println!();
    println!("To activate the hook, add the following to your agent configuration:");
    println!();
    match agent {
        AgentType::Claude => {
            println!("  Claude Code: Set the CLAUDE_HOOK environment variable:");
            println!("    export CLAUDE_HOOK={}", hook_path.display());
        }
        AgentType::Codex => {
            println!("  OpenAI Codex: Set the CODEX_HOOK environment variable:");
            println!("    export CODEX_HOOK={}", hook_path.display());
        }
        AgentType::Opencode => {
            println!("  Opencode: Set the OPCODE_HOOK environment variable:");
            println!("    export OPCODE_HOOK={}", hook_path.display());
        }
        // Unreachable: Pi returns early above (explicit `AgentType::Pi` branch).
        // The arm is retained only so the match stays exhaustive over
        // `AgentType`; it must never be reached.
        AgentType::Pi => unreachable!("Pi has no shell hook and returns early above"),
    }
    println!();
    println!("Or add the above line to your shell profile (~/.bashrc, ~/.zshrc, etc.)");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_type_as_str() {
        assert_eq!(AgentType::Claude.as_str(), "claude");
        assert_eq!(AgentType::Codex.as_str(), "codex");
        assert_eq!(AgentType::Opencode.as_str(), "opencode");
        assert_eq!(AgentType::Pi.as_str(), "pi");
    }

    #[test]
    fn test_agent_type_variants_distinct() {
        assert_ne!(AgentType::Claude, AgentType::Codex);
        assert_ne!(AgentType::Claude, AgentType::Opencode);
        assert_ne!(AgentType::Codex, AgentType::Opencode);
        assert_ne!(AgentType::Claude, AgentType::Pi);
        assert_ne!(AgentType::Codex, AgentType::Pi);
        assert_ne!(AgentType::Opencode, AgentType::Pi);
    }

    #[test]
    fn test_agent_type_parses_from_clap() {
        use clap::ValueEnum;

        assert_eq!(
            AgentType::from_str("pi", true).expect("pi must parse"),
            AgentType::Pi
        );
        assert_eq!(
            AgentType::from_str("claude", true).expect("claude must parse"),
            AgentType::Claude
        );
        assert_eq!(
            AgentType::from_str("codex", true).expect("codex must parse"),
            AgentType::Codex
        );
        assert_eq!(
            AgentType::from_str("opencode", true).expect("opencode must parse"),
            AgentType::Opencode
        );
        assert!(AgentType::from_str("nope", true).is_err());
    }

    #[test]
    fn test_agent_type_clap_help_mentions_pi() {
        use clap::ValueEnum;

        let names = AgentType::value_variants()
            .iter()
            .filter_map(|v| v.to_possible_value())
            .map(|v| v.get_name().to_string())
            .collect::<Vec<_>>();
        assert!(
            names.iter().any(|n| n == "pi"),
            "clap value enum must expose 'pi', got {names:?}"
        );
    }

    #[test]
    fn test_pi_has_no_shell_hook_script() {
        // pi loads extension packages, not shell hooks. The hook-script path
        // must be unavailable for Pi so we never emit a misleading script.
        assert!(AgentType::Pi.hook_script().is_none());

        // The other agents keep their shell hook.
        assert!(AgentType::Claude.hook_script().is_some());
        assert!(AgentType::Codex.hook_script().is_some());
        assert!(AgentType::Opencode.hook_script().is_some());
    }

    #[test]
    fn test_pi_has_no_hook_path() {
        // pi consumes an extension package, not a shell hook: there must be no
        // hook file path at all (not even a hypothetical `~/.pi/agent/terraphim-hook.sh`).
        assert!(
            AgentType::Pi.hook_path().is_none(),
            "pi must not expose a hook file path"
        );

        // The other agents keep their hook path.
        assert!(AgentType::Claude.hook_path().is_some());
        assert!(AgentType::Codex.hook_path().is_some());
        assert!(AgentType::Opencode.hook_path().is_some());
    }

    #[test]
    fn test_pi_config_dir_is_exactly_home_pi_agent() {
        let home = dirs::home_dir().expect("home dir must resolve");
        let expected = home.join(".pi").join("agent");

        let dir = AgentType::Pi
            .config_dir()
            .expect("pi config dir must resolve");

        assert_eq!(
            dir, expected,
            "pi config dir must be exactly <home>/.pi/agent, got {dir:?}"
        );
    }

    #[test]
    fn test_pi_install_guidance_points_at_package() {
        let guidance = pi_install_guidance();

        // The real contract is package installation.
        assert!(
            guidance.contains("packages/pi-terraphim-learn"),
            "guidance must point at the pi extension package, got: {guidance}"
        );
        assert!(
            guidance.contains("pi install"),
            "guidance must show the `pi install` command, got: {guidance}"
        );

        // The smoke helper from the package README.
        assert!(
            guidance.contains("terraphim-agent learn hook --format claude"),
            "guidance must include the smoke helper command, got: {guidance}"
        );

        // Must NOT claim pi uses the shell-hook activation path.
        assert!(
            !guidance.contains("PI_HOOK"),
            "guidance must not claim pi uses a PI_HOOK shell hook, got: {guidance}"
        );
        assert!(
            !guidance.contains("terraphim-hook.sh"),
            "guidance must not claim pi installs a shell hook script, got: {guidance}"
        );
    }

    #[test]
    fn test_pi_install_guidance_matches_package_readme() {
        let guidance = pi_install_guidance();

        // The version requirement must be stated at install time, not only in
        // the smoke-test step. Refs terraphim-clients#458.
        let install_step = guidance
            .split("1. Install the extension package")
            .nth(1)
            .expect("guidance must contain the install step")
            .split("2.")
            .next()
            .expect("install step must be delimited");
        assert!(
            install_step.contains("terraphim-agent >= 1.21.0"),
            "install step must state the terraphim-agent >= 1.21.0 requirement, got: {install_step}"
        );

        // `pi doctor` must be mentioned so users know how to acknowledge trust.
        assert!(
            guidance.contains("pi doctor"),
            "guidance must mention `pi doctor` for extension trust, got: {guidance}"
        );
        assert!(
            guidance.contains("trust"),
            "guidance must tell users to acknowledge extension trust, got: {guidance}"
        );

        // The smoke command must be the exact JSON envelope from the README.
        assert!(
            guidance.contains(
                r#"{"tool_name":"Bash","tool_input":{"command":"false"},"tool_result":{"exit_code":1,"stdout":"","stderr":"x"}}"#
            ),
            "guidance must include the exact smoke JSON envelope, got: {guidance}"
        );
    }

    #[test]
    fn test_hook_script_contains_agent_name() {
        let claude_script = AgentType::Claude.hook_script().expect("claude has a hook");
        assert!(claude_script.contains("terraphim-agent"));
        assert!(claude_script.contains("learn hook"));

        let codex_script = AgentType::Codex.hook_script().expect("codex has a hook");
        assert!(codex_script.contains("terraphim-agent"));
        assert!(codex_script.contains("learn hook"));

        let opencode_script = AgentType::Opencode
            .hook_script()
            .expect("opencode has a hook");
        assert!(opencode_script.contains("terraphim-agent"));
        assert!(opencode_script.contains("learn hook"));
    }

    #[test]
    fn test_hook_script_fail_open() {
        // All scripts should pass through unchanged if terraphim-agent fails
        let script = AgentType::Claude.hook_script().expect("claude has a hook");
        assert!(script.contains("2>/dev/null || true"));
        assert!(script.contains("cat"));
    }

    #[test]
    fn test_install_error_display() {
        let err = InstallError::ConfigNotFound;
        assert_eq!(err.to_string(), "agent config directory not found");

        let err = InstallError::ConfigDirError("permission denied".to_string());
        assert_eq!(
            err.to_string(),
            "failed to create config directory: permission denied"
        );
    }
}
