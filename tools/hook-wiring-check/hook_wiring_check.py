"""Read-only check: which hook entries the graphhelm plugin ships that a host settings file does not run.

Usage: python hook_wiring_check.py <settings.json>
Exit 0 nothing missing, 1 something missing, 2 settings file unreadable or not JSON.
An entry counts as run when the settings have the same event and script (or command text when the
shipped command names no .py script) under a matcher that covers every tool name of the shipped one.
Reads only the settings file and the shipped manifest; writes nothing, spawns nothing.
"""
import json
import re
import sys
from pathlib import Path

MANIFEST = Path(__file__).resolve().parents[2] / "plugins" / "graphhelm" / "hooks" / "hooks.json"
SCRIPT = re.compile(r"[\w.-]+\.py")
PLUGIN_NAME = "graphhelm"


def _command_text(hook: dict) -> str:
    return " ".join([str(hook.get("command", "")), *map(str, hook.get("args") or [])]).strip()


def _identities(hook: dict, shipped: bool) -> list[str]:
    """Script file names a hook runs; a shipped hook naming no script is identified by its command text.
    A settings hook also offers its command text, so such an entry can match."""
    text = _command_text(hook)
    scripts = sorted(set(SCRIPT.findall(text)))
    if shipped:
        return scripts or ([text] if text else [])
    return scripts + [text]


def _entries(hooks: object, shipped: bool) -> list[tuple[str, str, str]]:
    """(event, matcher, identity) for every hook in a `hooks` object; tolerant of odd shapes."""
    found = []
    if not isinstance(hooks, dict):
        return found
    for event, groups in hooks.items():
        for group in groups if isinstance(groups, list) else []:
            if not isinstance(group, dict):
                continue
            for hook in group.get("hooks") or []:
                if isinstance(hook, dict):
                    for identity in _identities(hook, shipped):
                        found.append((event, str(group.get("matcher") or ""), identity))
    return found


def _covers(settings_matcher: str, shipped_matcher: str) -> bool:
    """True when the settings matcher selects every tool name the shipped matcher selects.
    Empty or `*` selects all; otherwise matchers are compared as `|`-separated names."""
    if settings_matcher in ("", "*"):
        return True
    if shipped_matcher in ("", "*"):
        return False
    return set(shipped_matcher.split("|")) <= set(settings_matcher.split("|"))


def plugin_enabled(settings: dict) -> bool:
    enabled = settings.get("enabledPlugins")
    if not isinstance(enabled, dict):
        return False
    return any(key.split("@")[0] == PLUGIN_NAME and value is True for key, value in enabled.items())


def missing(settings: dict, manifest: dict) -> list[tuple[str, str, str]]:
    """Shipped entries the settings do not run; a settings file enabling the plugin runs all."""
    if plugin_enabled(settings):
        return []
    running = _entries(settings.get("hooks"), shipped=False)
    return [
        (event, matcher, identity)
        for event, matcher, identity in _entries(manifest.get("hooks"), shipped=True)
        if not any(e == event and i == identity and _covers(m, matcher) for e, m, i in running)
    ]


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: hook_wiring_check.py <settings.json>", file=sys.stderr)
        return 2
    try:
        settings = json.loads(Path(argv[1]).read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        print(f"settings file unreadable or not JSON: {type(error).__name__}", file=sys.stderr)
        return 2
    if not isinstance(settings, dict):
        print("settings file is JSON but not an object", file=sys.stderr)
        return 2
    try:
        manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        print(f"shipped manifest unreadable: {type(error).__name__}", file=sys.stderr)
        return 2
    lacking = missing(settings, manifest)
    for event, matcher, identity in lacking:
        print(f"missing: event={event} matcher={matcher or '-'} script={identity}")
    return 1 if lacking else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
