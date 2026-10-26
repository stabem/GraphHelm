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

The recursive Claude probe has one parent agent and one grandchild. The parent's
hand-back reports only its final turn (15 output tokens), while its two finalized
persisted messages total 167. The grandchild contributes 19. Together with the
main loop's 344, these reconcile with the pipeline's 530 output tokens. The
recursive fixtures retain native session, agent, message and version bindings;
they omit message bodies. The subscription fixture retains the native first-party
provider and explicit overage rejection, without account or billing identifiers.

These inclusion semantics are qualified for Codex 0.157.1 and Claude Code
2.1.310. A different version requires fresh native qualification. Parser replay
alone does not establish live-log ownership; the controller archives only the
captured session's logs outside the subject and checks their native provenance.
