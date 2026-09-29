---
name: graphhelm-guide
description: Use when the user asks how GraphHelm works, how Keel and Journey-Proven Development fit together, or how to start using its plugin, CLI, Runtime, and host setup.
---

# GraphHelm guide

Read the bundled [GraphHelm guide](../../README.md) and explain only the parts relevant to the user's question. Distinguish shipped behavior from product plans and host discovery from Runtime activation. When a claim depends on the current repository or a live execution, inspect that source before stating it as current.

Use the guide's left-to-right flow to explain how a user promise becomes a scoped change and a proof. For concrete code work, choose the smallest adequate route: Keel for scope and total delivery cost; Journey-Proven Development when the journey, observer, or risk needs explicit treatment. An unavailable observer remains unresolved. Skills advise; only GraphHelm's deterministic controls can validate and publish operational graph changes.

Installing this plugin exposes guidance. It does not install the CLI, start the Runtime, rewrite host customization, activate extension packages, or certify an execution. Explain the separate steps only when the user needs them.

## Prefer the GraphHelm MCP tools

When the `graphhelm` MCP server is connected, read live state through its tools rather than shelling out: `mcp__graphhelm__briefing`, `status`, `events`, `evidence`, `resume` and `memory_status`. If neither is connected, use the `graphhelm` CLI with `--json`, and say which source you used. `graphhelm init` registers a project-aware MCP command; `graphhelm setup --resolve home/.claude.json=register-mcp` can install one user-scope entry that resolves the active project by identity. Explicit `--url` and `--token-file` remain available for a deliberately pinned Runtime.
