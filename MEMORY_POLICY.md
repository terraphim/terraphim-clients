# Memory Policy for Terraphim AI

This document defines the boundary between **public commons memory** and **permissioned memory** in the Terraphim AI memory lifecycle system.

## Overview

The `terraphim-agent memory` CLI namespace implements the eight-stage agentic memory lifecycle: capture, distill, scope, provenance, retrieve, apply, validate, and retire. This policy ensures memory items are stored in the appropriate location with the correct access controls.

Vocabulary follows memco.ai's *Agentic Engineering Memory: Field Guide* (https://www.memco.ai/field-guide).

## Public Commons Memory

**Location:** terraphim-skills repository, Gitea wiki KG entries, shared automata thesauri.

**Characteristics:**
- Licensed under Apache-2.0
- Contains no personally identifiable information (PII)
- Contains no secrets, keys, or credentials
- Suitable for cross-project and cross-organisation sharing
- Published through Gitea wiki or terraphim-skills repo

**Examples:**
- General-purpose code patterns and best practices
- Shared automata thesauri for common domains
- Published KG entries for public knowledge
- Open-source skill definitions

## Permissioned Memory

**Location:** Per-project KGs under `kg/projects/<slug>/`, per-agent corrections, session transcripts.  
**Default storage:** `~/.config/terraphim/` or per-repo `.terraphim/` directory.

**Characteristics:**
- Contains project-specific knowledge
- May contain internal architecture details
- May contain agent-specific corrections and learnings
- Must never leave the device without explicit publication
- Guarded by `terraphim-agent memory scope --check`

**Examples:**
- Project-specific code patterns and conventions
- Agent learning corrections (failed commands, user preferences)
- Session transcripts from AI coding assistants
- Per-project KG entries with internal domain knowledge
- Per-agent evolution snapshots

## Enforcement

The `memory scope --check` command warns when a capture operation would write a permissioned item into a public location:

```bash
terraphim-agent memory scope --check
```

This scans `~/.config/terraphim/kg/` for project-specific KGs and verifies they are not in public locations.

## Storage Locations

| Memory Type | Location | Visibility |
|---|---|---|
| Captured learnings | `~/.config/terraphim/learnings/` | Permissioned |
| Compiled thesaurus | `~/.config/terraphim/cache/` | Permissioned |
| Role KGs | `~/.config/terraphim/kg/` | Permissioned |
| Project KGs | `~/.config/terraphim/kg/projects/<slug>/` | Permissioned |
| Session transcripts | `~/.config/terraphim/sessions/` | Permissioned |
| Evolution snapshots | `~/.config/terraphim/evolution/` | Permissioned |
| Published KGs | terraphim-skills repo, Gitea wiki | Public Commons |
| Shared automata thesauri | terraphim-skills repo | Public Commons |

## Responsibilities

- **Operators:** Run `memory scope --check` before publishing any KG or thesaurus.
- **Agents:** Write only to permissioned locations unless explicitly instructed to publish.
- **ADF (AI Dark Factory):** Automatically verify scope on capture; reject public writes for permissioned data.
- **CTO:** Approve all retirements and public commons publications.

## Related

- Research: `cto-executive-system/research/terraphim-ai-memory-lifecycle-research.md`
- Issue: https://git.terraphim.cloud/terraphim/terraphim-ai/issues/1899
- memco field guide: https://www.memco.ai/field-guide
