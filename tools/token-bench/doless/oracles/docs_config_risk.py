"""docs-config-risk: the Keel proportionality table in DELIVERY.md section 2 and in the AGENTS.md
Keel section keeps inert config on the light row but sends runtime- or security-affecting config to
the full card and behavioural evidence. (Fix 08dc05c4, its DELIVERY.md and AGENTS.md half.)

The behavioural-evidence requirement may sit on either routing row: the fix put it on the light
row, while the issue prompt only asks that such config "goes to the expanded route and needs
behavioural evidence", which an edit to the expanded row states as well (#139). The requirement may
also sit in the paragraph directly under the table, when that paragraph names config (#2141: four
chain runs wrote it there and failed a cell-only search)."""
import re

from _lib import fail, ok, read, section


def rows(text: str) -> list:
    return [re.sub(r"\s+", " ", l.lower()) for l in text.splitlines() if l.startswith("|")]


def paragraph_after_table(text: str) -> str:
    """The first paragraph after the first Markdown table in `text`, normalised like a row."""
    lines = text.splitlines()
    i = next((n for n, l in enumerate(lines) if l.startswith("|")), len(lines))
    while i < len(lines) and lines[i].startswith("|"):
        i += 1
    while i < len(lines) and not lines[i].strip():
        i += 1
    para = []
    while i < len(lines) and lines[i].strip() and not lines[i].startswith(("|", "#")):
        para.append(lines[i]); i += 1
    return re.sub(r"\s+", " ", " ".join(para).lower())


def problems(table_rows: list, adjacent: str = "") -> list:
    """What the table gets wrong; empty when it routes config by the behaviour it changes."""
    light = [r for r in table_rows if r.startswith("| docs")]
    heavy = [r for r in table_rows if "persistence" in r and "permissions" in r]
    if len(light) != 1 or len(heavy) != 1:
        return ["the light (docs) row or the expanded (persistence) row is missing"]
    found = []
    if "config" not in light[0] or "inert" not in light[0]:
        found.append("the light row does not limit config to inert values")
    if not ("runtime" in heavy[0] and "config" in heavy[0]):
        found.append("the expanded row does not include runtime-affecting config")
    prose = [adjacent] if "config" in adjacent else []
    if not any(re.search(r"behavio[u]?ral evidence", r) for r in (light[0], heavy[0], *prose)):
        found.append("no routing row says runtime-affecting config needs behavioural evidence")
    return found


def check(name: str, table_rows: list, adjacent: str = "") -> None:
    for problem in problems(table_rows, adjacent):
        fail(f"{name}: {problem}")


def main() -> None:
    delivery = section(read("docs/process/DELIVERY.md"), "## 2.")
    check("DELIVERY.md", rows(delivery), paragraph_after_table(delivery))
    agents = read("AGENTS.md")
    start = agents.find("| The change | What Keel asks |")
    if start < 0:
        fail("AGENTS.md: the Keel proportionality table is missing")
    check("AGENTS.md", rows(agents[start:].split("\n\n")[0]), paragraph_after_table(agents[start:]))
    ok("both tables route config by the behaviour it changes")


if __name__ == "__main__":
    main()
