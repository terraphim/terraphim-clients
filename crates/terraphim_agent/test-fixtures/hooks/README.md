# Hook-event payload fixtures

Real per-agent hook-event payloads used by the `learnings::hook` parser tests
(issue #2). These are captured from the actual deployed integrations, not
hand-fabricated mocks.

| Fixture | Source | Notes |
|---------|--------|-------|
| `claude_post_tool_use.json` | Claude Code `PostToolUse` hook | Confirmed schema: `tool_name` + `tool_input.command` + `tool_result.{exit_code,stdout,stderr}`. |
| `opencode_native_tool_execute_after.json` | opencode 1.15.13 plugin `tool.execute.after(input, output)` | Native envelope as read by the deployed `terraphim-hooks.js` plugin: `tool`, `args.command`, `output`, `metadata.exitCode`. |
| `opencode_normalised.json` | Deployed opencode plugin output | The plugin already normalises the native event to the Claude shape before invoking `terraphim-agent learn hook --format opencode`. |
| `codex_notify_turn_complete.json` | OpenAI Codex CLI 0.118.0 `notify` program argument | Turn-level `agent-turn-complete` event. Carries no per-command result, so it is intentionally non-capturing. |

The opencode native shape was captured from the deployed plugin at
`~/.config/opencode/plugin/terraphim-hooks.js`, which reads
`input.tool`, `output.args.command`, `output.output`, and
`output.metadata.exitCode` / `output.metadata.exit_code`.
