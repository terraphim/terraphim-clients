# Handover: terraphim-grep Model Patching Session

**Date**: 2026-07-14 16:46 BST
**Repository**: terraphim/terraphim-clients
**Branch**: `release/v1.21.10`
**Session Goal**: Fix terraphim-grep dropping LLM answers for models returning plain text instead of JSON

---

## 1. Progress Summary

### Tasks Completed

| Task | Status | Notes |
|------|--------|-------|
| Research OPENROUTER_MODEL settings and trade-offs | Done | Found default `qwen/qwen3-coder:free` rate-limited; identified alternatives |
| Check Gitea/GitHub for existing model-patching work | Done | Found issue #77 and existing local commits |
| Create clean PR branch from main | Done | `task/77-grep-model-answer-json` from `gitea/main` |
| Cherry-pick grep fixes | Done | `33063f9` + `f97c875` cherry-picked cleanly |
| Verify fix with multiple models | Done | `deepseek/deepseek-v4-flash` and `amazon/nova-micro-v1` return JSON answers |
| Address P2 review findings | Done | 3 findings fixed in commit `7a6d1a4` |
| Address P3 review findings | Done | 3 findings fixed in commit `d5972da` |
| Merge PR #78 (Gitea) | Done | Squash-merged; issue #77 closed |
| Merge PR #5 (GitHub) | Done | Squash-merged |
| Create release v1.21.10 | Done | Tag pushed; releases created on both platforms |
| Rebuild and reinstall locally | Done | `terraphim-grep 1.21.10` installed to `~/.cargo/bin/` |
| Verify with `deepseek/deepseek-v4-flash` | Done | Structured JSON answer with 28 citations, confidence 0.95 |
| Update cto-executive-system status | Done | `projects/terraphim-ai/status.md` updated |

### Current Implementation State

- **Working**: terraphim-grep `--answer` now returns structured JSON for models that previously returned plain text
- **Working**: Long-timeout OpenRouter client prevents 10s API timeout aborts
- **Working**: All 43 tests pass, clippy clean, fmt clean
- **Blocked**: `native-ci / build (push)` status check failing on main (pre-existing, not related to this change)
- **Pending**: Release binaries workflow running (https://github.com/terraphim/terraphim-clients/actions/runs/29341503886)

### What's Working

- `OPENROUTER_MODEL=deepseek/deepseek-v4-flash terraphim-grep "fn main" --haystack code --answer --json` returns proper JSON answer
- `OPENROUTER_MODEL=amazon/nova-micro-v1` also works reliably
- Free-tier models still rate-limited by Venice provider (429 errors)

### What's Blocked

- Native CI build failing on main (pre-existing issue; release-binaries workflow is independent)
- `meta-llama/llama-3.2-3b-instruct` still returns null (model ignores JSON instructions; acceptable)

---

## 2. Technical Context

```bash
# Current branch
git branch --show-current
# release/v1.21.10

# Recent commits
git log -5 --oneline
# 463d381 chore(release): bump workspace version to 1.21.10
# a9cf7b1 Fix #77: include JSON instructions in RLM prompt and tolerate markdown-wrapped output (#78)
# b49e96e docs: handover for R2 binary distribution migration (epic #62)
# 6107db0 fix(terraphim_update): name temp download after asset for zipsign filename context Refs #62
# 3a146ad feat(terraphim_update): hard-reject unsigned archives (MissingSignature->Failed) Refs #62

# Modified files (this session)
git status --short
# (clean - all committed and pushed)
```

### Key Files Changed

| File | Changes |
|------|---------|
| `crates/terraphim_grep/src/lib.rs` | Append `AnswerSignature::instructions()` to RLM prompt; add prompt assembly tests |
| `crates/terraphim_grep/src/signatures.rs` | Add `extract_json()` helper; add edge-case tests |
| `crates/terraphim_grep/src/openrouter_client.rs` | New long-timeout client; error taxonomy fix; construction tests |
| `crates/terraphim_grep/src/main.rs` | Prefer long-timeout client; fall back to `role_from_env`; free-tier warning |
| `crates/terraphim_grep/Cargo.toml` | Make `reqwest` optional (tied to `llm` feature) |
| `Cargo.toml` | Bump workspace version to 1.21.10 |

### Environment

- `OPENROUTER_API_KEY`: set (from 1Password)
- `OPENROUTER_MODEL`: not set globally; recommended `deepseek/deepseek-v4-flash` or `amazon/nova-micro-v1`
- `terraphim-grep --version`: 1.21.10

### Model Recommendations (from trade-off analysis)

| Model | Speed | Cost | Reliability | Use Case |
|-------|-------|------|-------------|----------|
| `amazon/nova-micro-v1` | ~11s | Very low | Works reliably | Daily driver |
| `deepseek/deepseek-v4-flash` | ~5.6s | Very low | Works after fix | Fast synthesis |
| `mistralai/mistral-nemo` | ~31s | Lowest | Works reliably | High-quality, patient |
| `qwen/qwen3-coder:free` | — | Free | Rate-limited | Smoke tests only |
| ~~`openai/gpt-oss-20b:free`~~ | ~20s+ | Free | Reasoning-only | Avoid |

---

## 3. Artefacts

- **Gitea PR**: https://git.terraphim.cloud/terraphim/terraphim-clients/pulls/78 (merged)
- **GitHub PR**: https://github.com/terraphim/terraphim-clients/pull/5 (merged)
- **Gitea Release**: https://git.terraphim.cloud/terraphim/terraphim-clients/releases/tag/v1.21.10
- **GitHub Release**: https://github.com/terraphim/terraphim-clients/releases/tag/v1.21.10
- **Issue**: https://git.terraphim.cloud/terraphim/terraphim-clients/issues/77 (closed)
- **cto-executive-system**: `projects/terraphim-ai/status.md` updated
- **Benchmark analysis**: `terraphim-ai` branch `task/3098-doc-grep-openrouter-rlm`

---

## 4. Next Actions

1. **Monitor release binaries workflow**: https://github.com/terraphim/terraphim-clients/actions/runs/29341503886
2. **Re-run model benchmark**: `python3 scripts/benchmark_openrouter_tradeoff/benchmark.py` after fix to update recommendations
3. **Upstream timeout config**: Make `terraphim_service` timeout configurable to remove client duplication
4. **Fix native CI build**: Investigate pre-existing `native-ci / build (push)` failure on main

---

## 5. Resume Steps

```bash
cd /Users/alex/projects/terraphim/terraphim-clients
git checkout release/v1.21.10

# Verify local install
terraphim-grep --version
# terraphim-grep 1.21.10

# Test with recommended model
export OPENROUTER_MODEL=deepseek/deepseek-v4-flash
terraphim-grep "fn main" --haystack code --answer --json

# Check release status
gh release view v1.21.10 --repo terraphim/terraphim-clients
```
