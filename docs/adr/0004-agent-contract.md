# ADR 0004: Agent-native contract (JSON, skill, MCP)

Status: accepted (2026-11-06)

## Context
Agents (Claude Code, Codex, Cursor, Copilot...) drive CLIs through text. Convergent 2026
practice: `--json` everywhere, stdout for data / stderr for humans, stable exit codes,
SKILL.md for discovery, MCP as a complement.

## Decision
1. `--json` on every command, one `kiwa/v1` envelope on stdout, errors with `kind` and
   `suggestions`. Documented in `docs/reference/json-and-exit-codes.md`.
2. Ship `skills/kiwa/SKILL.md`; `kw agent skill` installs it (v0.1).
3. `kw mcp` stdio server reusing kiwa-core (v0.1): `list_projects`, `get_project`,
   `run_task` (dry-run unless `confirm: true`).
4. Safety: fuzzy never selects what runs; destructive task names require confirmation
   from agents; per-root trust policy.

## Consequences
+ One implementation, three surfaces (CLI, skill, MCP).
+ "Safe for agents" is a differentiator few tools claim.
- The JSON shape is a public API: changes need changelog entries, and v2 after 1.0.
