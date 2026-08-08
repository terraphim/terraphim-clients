/**
 * pi-terraphim-learn — Terraphim learn capture for pi (pi_agent_rust)
 *
 * Install:
 *   pi install /path/to/terraphim-clients/packages/pi-terraphim-learn
 *
 * Listens for onToolResult, normalizes bash failures to Claude learn envelope,
 * pipes to: terraphim-agent learn hook --format claude --learn-hook-type post-tool-use
 *
 * Fail-open: missing agent or parse errors never block the session.
 */
import { spawn } from "node:child_process";

function runLearnHook(payload) {
  return new Promise((resolve) => {
    try {
      const child = spawn(
        "terraphim-agent",
        ["learn", "hook", "--format", "claude", "--learn-hook-type", "post-tool-use"],
        { stdio: ["pipe", "ignore", "ignore"] }
      );
      child.on("error", () => resolve());
      child.on("close", () => resolve());
      child.stdin.write(JSON.stringify(payload));
      child.stdin.end();
      // Don't hang the agent forever
      setTimeout(() => {
        try {
          child.kill("SIGKILL");
        } catch {
          /* ignore */
        }
        resolve();
      }, 5000);
    } catch {
      resolve();
    }
  });
}

function extractBashFailure(event) {
  // Defensive: pi event shapes vary by version
  const e = event || {};
  const tool =
    e.toolName || e.tool_name || e.name || e.tool || e.type || "";
  const isBash =
    String(tool).toLowerCase() === "bash" ||
    String(tool).toLowerCase() === "shell" ||
    String(tool).toLowerCase() === "tool_call" &&
      String(e.tool || e.name || "").toLowerCase() === "bash";

  const cmd =
    e.command ||
    e.args?.command ||
    e.input?.command ||
    e.params?.command ||
    e.toolInput?.command ||
    null;

  const exit =
    e.exitCode ??
    e.exit_code ??
    e.result?.exitCode ??
    e.result?.exit_code ??
    e.metadata?.exitCode ??
    e.metadata?.exit_code ??
    (e.isError || e.error ? 1 : 0);

  const stdout = e.stdout || e.result?.stdout || e.output || "";
  const stderr =
    e.stderr || e.result?.stderr || e.errorMessage || e.error || "";

  if (!cmd) return null;
  // Capture only failures; if we can't tell, skip unless stderr looks failed
  const code = Number(exit) || 0;
  if (code === 0 && !stderr) return null;

  // If tool name missing but command present and non-zero, still capture
  if (!isBash && tool && String(tool).toLowerCase() !== "bash") {
    // allow empty tool name with command
    if (tool) return null;
  }

  return {
    tool_name: "Bash",
    tool_input: { command: String(cmd) },
    tool_result: {
      exit_code: code === 0 && stderr ? 1 : code,
      stdout: String(stdout).slice(0, 8000),
      stderr: String(stderr).slice(0, 8000),
    },
  };
}

export default function activate(pi) {
  if (!pi || typeof pi.on !== "function") {
    return;
  }

  pi.on("onToolResult", async (event) => {
    try {
      const payload = extractBashFailure(event);
      if (!payload) return;
      await runLearnHook(payload);
    } catch {
      /* fail-open */
    }
  });
}
