# Design Gate — pi-rust learn hooks (Phase 2 multi-client)

**Date:** 2026-08-08  
**Issue/plan:** `2026-08-08-learn-hooks-multi-client.md` Phase 2  
**Repo:** terraphim-clients (+ installable package for `pi install`)

## Problem
pi (pi_agent_rust) has no Terraphim learn/replace/guard wiring. Claude and OpenCode are Phase 0 done; pi is the gap.

## Decision
**Approach A:** JS extension package (not Rust fork interceptor).

### Touchpoints
1. **NEW** `packages/pi-terraphim-learn/`  
   - `index.js` — `export default function (pi) { pi.on("onToolResult", ...); }`  
   - Optional: message/input event for user-prompt-submit if available  
   - `package.json` / README for `pi install <path>`  
2. **EDIT** `crates/terraphim_agent/src/learnings/install.rs`  
   - `AgentType::Pi`  
   - `hook_script()` documents install path (pi uses packages, not shell in ~/.claude)  
   - `config_dir()` → `~/.pi/agent`  
3. **EDIT** `AgentFormat` only if needed — prefer normalize to Claude/opencode envelope in the extension and call `learn hook --format auto`

### Event contract (from pi docs/ext-compat.md)
- `pi.on("onToolResult", async (event) => { ... })` after tool runs  
- Host tools: `pi.tool("bash", …)` / built-in bash  
- Extension must **fail-open** if `terraphim-agent` missing  
- Prefer `pi.exec` only for spawning agent CLI with stdin JSON

### Envelope mapping (in extension)
```js
// after onToolResult — shape may vary; normalize defensively
{
  tool_name: "Bash",
  tool_input: { command },
  tool_result: { exit_code, stdout, stderr }
}
→ terraphim-agent learn hook --format claude --learn-hook-type post-tool-use
```

Pre-tool: if pi exposes before-tool event, mirror OpenCode before (guard/replace/learn-pre). If only onToolResult, ship **post-only** first (capture), document pre as follow-up.

### Acceptance
1. `AgentType::Pi` in install enum + tests  
2. Package loads: `pi doctor packages/pi-terraphim-learn` (or install) without hard fail  
3. Documented smoke: failed bash → learning file when agent on PATH  
4. No secrets in logs; fail-open  

### Out of scope
- Correction→KG compile (#810 P3)  
- Hard-block guard on pi (advisory only v1)  
- Merging into pi_agent_rust upstream  

### Test plan
- Unit: install.rs Pi variant  
- Manual/script: pipe synthetic onToolResult-equivalent JSON through agent  
- `pi doctor` on package path if available
