# GraphHelm — naming decision

## Status

**Product name selected for development:** `GraphHelm`.

This decision sets the working name for the framework, the Runtime, and the Studio. It does not replace trademark legal research, domain acquisition, or namespace reservation on registries before public launch.

## Why GraphHelm

- **Graph** represents the product's central abstraction: every request is compiled into an executable, adaptive, versioned, and editable graph.
- **Helm** represents direction and sovereignty: the harness governs execution by default, while the user remains in command and can alter the flow.
- The name works for software, research, automation, documents, operations, and other domains; it does not limit the system to coding agents.
- It is short, pronounceable, technical, and suitable for a global open source framework.

## Positioning

**Category:** open-source agent operating system and control plane.

**Main tagline:**

> The open-source control plane for governed AI agents.

**Short product line:**

> Compose agents. Govern every run.

**One-sentence description:**

> GraphHelm dynamically compiles, executes, governs, and visualizes task-specific AI agent graphs on infrastructure controlled by the user.

## Brand architecture

- `GraphHelm Core` — Graph Engine, Harness Compiler, Policy Engine, and protocols.
- `GraphHelm Runtime` — daemon running on the user's VPS.
- `GraphHelm Studio` — local interface for chat, graph, execution, and documentation.
- `GraphHelm CLI` — automatable interface for projects, graphs, and runtime.
- `GraphHelm SDK` — TypeScript and Python SDKs.
- `GraphHelm Registry` — open, replaceable catalog of extensions, skills, tools, and schemas.

## Proposed technical conventions

These conventions should only be published after the respective namespaces are reserved:

```text
GitHub organization: graphhelm
CLI command: graphhelm
JavaScript scope: @graphhelm/*
Rust crates: graphhelm-*
Python packages: graphhelm-*
Project directory: .graphhelm/
Primary config: graphhelm.yaml
```

Existing wire-format identifiers with `p50.dev` remain provisionally valid until a specific ADR approves the namespace migration. The first implementation must not silently change public identifiers.

## Checklist before public announcement

1. trademark research in relevant jurisdictions;
2. reservation of the primary domain and defensive variants;
3. reservation of the GitHub organization;
4. reservation of npm, PyPI, and crates.io namespaces;
5. research of similar names in agent, graph, DevTools, and Kubernetes projects;
6. ADR for migrating from `p50.dev` to the definitive namespace;
7. atomic update of schemas, examples, documentation, and generated contracts.
