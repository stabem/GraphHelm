"""docs-config-risk: the Keel proportionality table in DELIVERY.md section 2 and in the AGENTS.md
Keel section keeps inert config on the light row but sends runtime- or security-affecting config to
the full card and behavioural evidence. (Fix 08dc05c4, its DELIVERY.md and AGENTS.md half.)"""
import re

from _lib import fail, ok, read, section


def rows(text: str) -> list:
    return [re.sub(r"\s+", " ", l.lower()) for l in text.splitlines() if l.startswith("|")]


def check(name: str, table_rows: list) -> None:
    light = [r for r in table_rows if r.startswith("| docs")]
    heavy = [r for r in table_rows if "persistence" in r and "permissions" in r]
    if len(light) != 1 or len(heavy) != 1:
        fail(f"{name}: the light (docs) row or the expanded (persistence) row is missing")
    if "config" not in light[0] or "inert" not in light[0]:
        fail(f"{name}: the light row does not limit config to inert values")
    if not ("runtime" in light[0] and "behavio" in light[0]):
        fail(f"{name}: the light row does not say runtime-affecting config needs behavioural evidence")
    if not re.search(r"runtime[^|]*config", heavy[0]):
        fail(f"{name}: the expanded row does not include runtime-affecting config")


check("DELIVERY.md", rows(section(read("docs/process/DELIVERY.md"), "## 2.")))
agents = read("AGENTS.md")
start = agents.find("| The change | What Keel asks |")
if start < 0:
    fail("AGENTS.md: the Keel proportionality table is missing")
check("AGENTS.md", rows(agents[start:].split("\n\n")[0]))
ok("both tables route config by the behaviour it changes")
