# pi-terraphim-learn

Terraphim **learn capture** extension for [pi](https://github.com/terraphim/pi_agent_rust) (`pi_agent_rust`).

## Install

```bash
# requires terraphim-agent >= 1.21.0 on PATH
pi install /path/to/terraphim-clients/packages/pi-terraphim-learn
# or from a checkout:
pi install ~/projects/terraphim-clients/packages/pi-terraphim-learn
```

Acknowledge extension trust if `pi doctor` prompts.

## Behaviour

| Event | Action |
|-------|--------|
| `onToolResult` | If bash-like command failed → `terraphim-agent learn hook --format claude --learn-hook-type post-tool-use` |

Fail-open: missing agent, parse errors, or timeouts never block pi.

## Smoke without pi

```bash
echo '{"tool_name":"Bash","tool_input":{"command":"false"},"tool_result":{"exit_code":1,"stdout":"","stderr":"x"}}' \
  | terraphim-agent learn hook --format claude
ls -lt ~/.local/share/terraphim/learnings/ | head
```

## Related

- Multi-client plan: `cto-executive-system/2026-08-08-learn-hooks-multi-client.md`
- Design: `docs/plans/design-pi-terraphim-learn-2026-08-08.md`
- CLI: `terraphim-agent learn install-hook pi`
