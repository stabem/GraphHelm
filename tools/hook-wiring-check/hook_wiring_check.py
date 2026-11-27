"""Read-only check: which hook entries the graphhelm plugin ships that a host settings file does not run.

Usage: python hook_wiring_check.py <settings.json>
Exit 0 nothing missing, 1 something missing, 2 settings file unreadable or not JSON.
Reads only the settings file and the shipped manifest; writes nothing, spawns nothing.
"""
import json
import re
import sys
from pathlib import Path

MANIFEST = Path(__file__).resolve().parents[2] / "plugins" / "graphhelm" / "hooks" / "hooks.json"
SCRIPT = re.compile(r"[\w.-]+\.py")
PLUGIN_NAME = "graphhelm"


def _scripts(hook: dict) -> set[str]:
    """Script file names a hook entry runs, from its command and args text."""
    text = " ".join([str(hook.get("command", "")), *map(str, hook.get("args") or [])])
    return set(SCRIPT.findall(text))


def _entries(hooks: object) -> list[tuple[str, str, str]]:
    """(event, matcher, script) for every hook in a `hooks` object; tolerant of odd shapes."""
    found = []
    if not isinstance(hooks, dict):
        return found
    for event, groups in hooks.items():
        for group in groups if isinstance(groups, list) else []:
            if not isinstance(group, dict):
                continue
            for hook in group.get("hooks") or []:
                if isinstance(hook, dict):
                    for script in sorted(_scripts(hook)):
                        found.append((event, str(group.get("matcher") or ""), script))
    return found


def plugin_enabled(settings: dict) -> bool:
    enabled = settings.get("enabledPlugins")
    if not isinstance(enabled, dict):
        return False
    return any(key.split("@")[0] == PLUGIN_NAME and value is True for key, value in enabled.items())


def missing(settings: dict, manifest: dict) -> list[tuple[str, str, str]]:
    """Shipped entries the settings do not run; a settings file enabling the plugin runs all."""
    if plugin_enabled(settings):
        return []
    running = {(event, script) for event, _, script in _entries(settings.get("hooks"))}
    return [e for e in _entries(manifest.get("hooks")) if (e[0], e[2]) not in running]


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
    for event, matcher, script in lacking:
        print(f"missing: event={event} matcher={matcher or '-'} script={script}")
    return 1 if lacking else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
