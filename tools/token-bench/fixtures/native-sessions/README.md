# Native subscription accounting probes

These JSONL excerpts were captured on 2026-10-26 with Codex CLI 0.157.1 and
Claude Code 2.1.310 using subscription authentication. They retain only native
session metadata, cumulative counters, successful spawn linkage, Agent tool
result counters and final pipeline counters. Instructions, model text, account
identifiers and authentication data are omitted. They are parser evidence, not
evidence that any benchmark task or complete development workflow succeeded.

Codex's second child used `fork_turns=all`. Its log embeds ancestor metadata
before its own `thread_settings_applied` boundary. The child's own counters are
additional to its parent's counters. Claude's pipeline counters already include
the child's completed tool-result usage; forwarded assistant text reported a
different provisional output count and is not an accounting source.
