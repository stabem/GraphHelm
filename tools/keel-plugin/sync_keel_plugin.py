"""Writes the standalone keel plugin's skills from the repository's KEEL files, which stay the only source."""
import json
import posixpath
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SKILLS_DIR = ROOT / "plugins" / "keel" / "skills"
# The development-contracts `keel` skill must stay self-contained, so it embeds one KEEL file.
GRAPHHELM_KEEL_SKILL = ROOT / "extensions" / "builtin" / "graphhelm-development-contracts" / "skills" / "keel" / "SKILL.md"
GRAPHHELM_KEEL_SOURCE = "keel/KEEL_EXEC.md"

# skill folder -> the KEEL file it carries, and when a host should load it.
SKILLS = {
    "keel": "KEEL.md",
    "keel-plan": "keel/KEEL_PLAN.md",
    "keel-exec": "keel/KEEL_EXEC.md",
    "keel-review": "keel/KEEL_REVIEW.md",
    "keel-merge": "keel/KEEL_MERGE.md",
}
# Skills written by hand in the plugin, carrying no KEEL file; the sync never touches them.
HAND_WRITTEN = ("keel-journey-qa",)
DESCRIPTIONS = {
    "keel": "Use before any code work in a project that follows Keel: the values every step applies "
            "(read cost, end-to-end proof, cheap-to-change code) and which step file to load next.",
    "keel-plan": "Use when planning a code change under Keel, before writing code: the contract card, "
                 "the paths in scope, the promise and how it will be proven.",
    "keel-exec": "Use while implementing a planned change under Keel: how to write within the card, "
                 "prove the promise end to end, break it once on purpose and hand off.",
    "keel-review": "Use when reviewing someone else's change under Keel as its single blind reviewer: "
                   "what to read, what to run and how to give the verdict.",
    "keel-merge": "Use when merging a change you approved under Keel: what to recheck at the merge and "
                  "what to record after it.",
}
# A relative link copied out of the repository leads nowhere inside a plugin, so it points here.
GITHUB_BLOB = "https://github.com/stabem/GraphHelm/blob/main/"
RELATIVE_LINK = re.compile(r"\]\((?![a-z][a-z0-9+.-]*:|#)([^)\s#]+)(#[^)\s]*)?\)")
SETUP_FILES = ROOT / "plugins" / "keel" / "setup" / "files"
PREAMBLE = """How to use this skill:

- First run: if the project has no `KEEL.md` at its root, install Keel into it with
  `python <this plugin>/setup/keel_setup.py <project root>` (the plugin folder is two levels above
  this skill's folder). It copies `KEEL.md` and the `keel/` step files into the project, offline,
  never overwrites an existing one, and adds a marked pointer line to `CLAUDE.md` and `AGENTS.md`.
- Read the project's own `KEEL.md` at its root when one exists, and the `keel/` step files it names.
  Before that step has run, read https://github.com/stabem/GraphHelm/blob/main/KEEL.md and the step
  files under https://github.com/stabem/GraphHelm/blob/main/keel/. The text below is the bundled copy.
- In the text below, `keel/KEEL_PLAN.md`, `keel/KEEL_EXEC.md`, `keel/KEEL_REVIEW.md` and
  `keel/KEEL_MERGE.md` are this plugin's `keel-plan`, `keel-exec`, `keel-review` and `keel-merge`
  skills, and `KEEL.md` is its `keel` skill. `docs/keel/KEEL_SPEC.md` exists only in the GraphHelm
  repository.
- If the `graphhelm` CLI is on PATH, run `graphhelm keel check --diff <base>..<head> --card <card>`
  for the rules marked as measured. If it is not, the reviewer checks those rules by reading and says
  so in the review.
- Nothing here needs a GraphHelm Runtime, MCP server, task records or journeys. Where the text names
  them, they apply only inside GraphHelm: elsewhere, prove the promise with the project's own test
  runner.
"""


def marker(source):
    return f"\n<!-- keel source: {source} (generated; edit the source, then run tools/keel-plugin/sync_keel_plugin.py) -->\n\n"


def github_links(text, source):
    # Every relative link in a KEEL file is resolved against that file and pointed at GitHub.
    base = posixpath.dirname(source)
    return RELATIVE_LINK.sub(
        lambda m: f"]({GITHUB_BLOB}{posixpath.normpath(posixpath.join(base, m.group(1)))}{m.group(2) or ''})", text)


def keel_text(source):
    return (ROOT / source).read_bytes().decode("utf-8").replace("\r\n", "\n")


def embedded_markers(source):
    return (f"<!-- keel embed start: {source} (generated; edit the source, then run tools/keel-plugin/sync_keel_plugin.py) -->\n",
            f"<!-- keel embed end: {source} -->\n")


def embed(skill_text, source):
    # Replaces only what lies between the markers; the hand-written rest of the skill is kept.
    start, end = embedded_markers(source)
    head, rest = skill_text.split(start, 1)
    tail = rest.split(end, 1)[1]
    return head + start + keel_text(source) + end + tail


def render(skill):
    source = SKILLS[skill]
    body = github_links(keel_text(source), source)
    frontmatter = f"---\nname: {skill}\ndescription: {json.dumps(DESCRIPTIONS[skill])}\n---\n\n"
    return frontmatter + PREAMBLE + marker(source) + body


def write_text(target, text):
    # A file whose text already matches keeps its bytes, so a CRLF checkout (core.autocrlf) stays
    # clean (#1459). Returns whether it wrote.
    if target.exists() and target.read_bytes().decode("utf-8").replace("\r\n", "\n") == text:
        return False
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(text.encode("utf-8"))
    return True


def main():
    for skill in SKILLS:
        write_text(SKILLS_DIR / skill / "SKILL.md", render(skill))
    for source in SKILLS.values():
        # The setup step ships the KEEL files as they are: inside a project their relative links hold.
        write_text(SETUP_FILES / source, keel_text(source))
    current = GRAPHHELM_KEEL_SKILL.read_bytes().decode("utf-8").replace("\r\n", "\n")
    write_text(GRAPHHELM_KEEL_SKILL, embed(current, GRAPHHELM_KEEL_SOURCE))
    print(f"wrote {len(SKILLS)} skills under {SKILLS_DIR.relative_to(ROOT).as_posix()}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
