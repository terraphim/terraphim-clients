/**
 * Functional test for the pi-terraphim-learn package (#467).
 *
 * The package is a pi (pi_agent_rust) plugin: `activate(pi)` registers an
 * `onToolResult` handler that normalises a bash failure into the Claude learn
 * envelope and pipes it to `terraphim-agent learn hook`.
 *
 * The test drives the real plugin end to end against a fake `terraphim-agent`
 * on `PATH`, so it proves the envelope the agent receives - not just that the
 * module parses.
 */
import test from "node:test";
import assert from "node:assert/strict";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const PACKAGE_URL = new URL("./index.js", import.meta.url);

/** A fake `terraphim-agent` that records the envelope it was given. */
function fakeAgentOnPath() {
  const root = mkdtempSync(join(tmpdir(), "pi-terraphim-learn-test-"));
  const capture = join(root, "captured.jsonl");
  const binDir = join(root, "bin");
  const bin = join(binDir, "terraphim-agent");
  mkdirSync(binDir, { recursive: true });
  writeFileSync(
    bin,
    `#!/bin/sh\ncat >> "${capture}"\nexit 0\n`,
    { mode: 0o755 },
  );
  chmodSync(bin, 0o755);
  return {
    capture,
    root,
    restore() {
      rmSync(root, { recursive: true, force: true });
    },
  };
}

/** Register the plugin on a fake `pi` and return the handlers it installed. */
async function activatePlugin() {
  const { default: activate } = await import(PACKAGE_URL);
  const handlers = new Map();
  activate({
    on(event, handler) {
      handlers.set(event, handler);
    },
  });
  return handlers;
}

test("a bash failure is piped to the agent as a Claude learn envelope", async () => {
  const agent = fakeAgentOnPath();
  const originalPath = process.env.PATH;
  process.env.PATH = `${join(agent.root, "bin")}:${originalPath}`;
  try {
    const handlers = await activatePlugin();
    const onToolResult = handlers.get("onToolResult");
    assert.ok(onToolResult, "the plugin must register an onToolResult handler");

    await onToolResult({
      toolName: "bash",
      command: "cargo build",
      exitCode: 1,
      stdout: "partial output",
      stderr: "error: could not compile",
    });

    const captured = readFileSync(agent.capture, "utf8").trim();
    assert.notEqual(captured, "", "the agent must receive an envelope");

    const envelope = JSON.parse(captured);
    assert.equal(envelope.tool_name, "Bash");
    assert.equal(envelope.tool_input.command, "cargo build");
    assert.equal(envelope.tool_result.exit_code, 1);
    assert.match(envelope.tool_result.stderr, /could not compile/);
  } finally {
    process.env.PATH = originalPath;
    agent.restore();
  }
});

test("a successful command is not captured", async () => {
  const agent = fakeAgentOnPath();
  const originalPath = process.env.PATH;
  process.env.PATH = `${join(agent.root, "bin")}:${originalPath}`;
  try {
    const handlers = await activatePlugin();
    await handlers.get("onToolResult")({
      toolName: "bash",
      command: "cargo build",
      exitCode: 0,
      stdout: "ok",
      stderr: "",
    });

    let captured = "";
    try {
      captured = readFileSync(agent.capture, "utf8");
    } catch {
      captured = "";
    }
    assert.equal(captured, "", "a successful command must not be captured");
  } finally {
    process.env.PATH = originalPath;
    agent.restore();
  }
});

test("a non-bash tool is not captured", async () => {
  const agent = fakeAgentOnPath();
  const originalPath = process.env.PATH;
  process.env.PATH = `${join(agent.root, "bin")}:${originalPath}`;
  try {
    const handlers = await activatePlugin();
    await handlers.get("onToolResult")({
      toolName: "read_file",
      command: "cat file",
      exitCode: 1,
      stdout: "",
      stderr: "missing",
    });

    let captured = "";
    try {
      captured = readFileSync(agent.capture, "utf8");
    } catch {
      captured = "";
    }
    assert.equal(captured, "", "only bash failures are captured");
  } finally {
    process.env.PATH = originalPath;
    agent.restore();
  }
});

test("the plugin fails open when the agent binary is missing", async () => {
  const originalPath = process.env.PATH;
  // An empty PATH means the spawn fails; the plugin must still resolve.
  process.env.PATH = join(tmpdir(), "pi-terraphim-learn-empty-path");
  try {
    const handlers = await activatePlugin();
    await handlers.get("onToolResult")({
      toolName: "bash",
      command: "false",
      exitCode: 1,
      stdout: "",
      stderr: "boom",
    });
  } finally {
    process.env.PATH = originalPath;
  }
});
