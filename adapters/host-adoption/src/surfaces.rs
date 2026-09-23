//! Explicit, bounded adoption surfaces shared by inventory, backup and restore.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Surface {
    pub id: &'static str,
    pub scope: &'static str,
    pub path: &'static str,
    pub host: &'static str,
    pub kind: &'static str,
}

/// Claude Code reads project instructions from `CLAUDE.md` or `.claude/CLAUDE.md`, a
/// git-ignored `CLAUDE.local.md` beside them, and user instructions from `~/.claude/CLAUDE.md`
/// (code.claude.com/docs/en/memory, checked 2026-09-22). `~/CLAUDE.md` is not a documented
/// location and is not a surface.
pub(crate) const ALL: [Surface; 16] = [
    Surface {
        id: "project/AGENTS.md",
        scope: "project",
        path: "AGENTS.md",
        host: "codex",
        kind: "instructions",
    },
    Surface {
        id: "project/CLAUDE.md",
        scope: "project",
        path: "CLAUDE.md",
        host: "claude",
        kind: "instructions",
    },
    Surface {
        id: "project/.claude/CLAUDE.md",
        scope: "project",
        path: ".claude/CLAUDE.md",
        host: "claude",
        kind: "instructions",
    },
    Surface {
        id: "project/CLAUDE.local.md",
        scope: "project",
        path: "CLAUDE.local.md",
        host: "claude",
        kind: "instructions",
    },
    Surface {
        id: "project/.mcp.json",
        scope: "project",
        path: ".mcp.json",
        host: "claude",
        kind: "mcp",
    },
    Surface {
        id: "project/.claude/settings.json",
        scope: "project",
        path: ".claude/settings.json",
        host: "claude",
        kind: "settings",
    },
    Surface {
        id: "project/.claude/settings.local.json",
        scope: "project",
        path: ".claude/settings.local.json",
        host: "claude",
        kind: "settings",
    },
    Surface {
        id: "project/.codex/config.toml",
        scope: "project",
        path: ".codex/config.toml",
        host: "codex",
        kind: "settings",
    },
    Surface {
        id: "project/AGENTS.override.md",
        scope: "project",
        path: "AGENTS.override.md",
        host: "codex",
        kind: "instructions",
    },
    Surface {
        id: "home/.claude/settings.json",
        scope: "home",
        path: ".claude/settings.json",
        host: "claude",
        kind: "settings",
    },
    Surface {
        id: "home/.claude.json",
        scope: "home",
        path: ".claude.json",
        host: "claude",
        kind: "mcp",
    },
    Surface {
        id: "home/.codex/config.toml",
        scope: "home",
        path: ".codex/config.toml",
        host: "codex",
        kind: "settings",
    },
    Surface {
        id: "home/AGENTS.md",
        scope: "home",
        path: "AGENTS.md",
        host: "codex",
        kind: "instructions",
    },
    Surface {
        id: "home/AGENTS.override.md",
        scope: "home",
        path: "AGENTS.override.md",
        host: "codex",
        kind: "instructions",
    },
    Surface {
        id: "home/.claude/CLAUDE.md",
        scope: "home",
        path: ".claude/CLAUDE.md",
        host: "claude",
        kind: "instructions",
    },
    Surface {
        id: "home/.claude/managed-settings.json",
        scope: "home",
        path: ".claude/managed-settings.json",
        host: "claude",
        kind: "managed_settings",
    },
];

pub(crate) fn for_host(host: &str) -> impl Iterator<Item = Surface> {
    ALL.into_iter().filter(move |surface| surface.host == host)
}

pub(crate) fn by_id(id: &str) -> Option<Surface> {
    ALL.into_iter().find(|surface| surface.id == id)
}
