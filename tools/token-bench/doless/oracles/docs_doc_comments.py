"""docs-doc-comments: each displaced doc block sits directly above the item it documents again
(`addressable_scope`, `repository_scope`, `synthesize_schema`), and no line other than a `///`
comment changed in either file. (Fix 28eb9c6a.)"""
from _lib import fail, ok, read, repo_show

PARENT = "25b34edc900699548e15beaba489f94693af84e3"
CASES = {
    "apps/cli/src/commands/execution/mod.rs": {
        "The ONE scope the `execution` verbs can address": "pub(crate) fn addressable_scope(",
        "The same addressing rule as [`addressable_scope`]": "pub(crate) fn repository_scope(",
    },
    "apps/cli/src/commands/mcp/tools.rs": {
        "Closed like every other schema here.": "fn synthesize_schema(",
    },
}


def code_lines(text: str) -> list:
    return [l.rstrip() for l in text.splitlines() if l.strip() and not l.strip().startswith("///")]


for path, blocks in CASES.items():
    text = read(path)
    lines = text.splitlines()
    for first_words, item in blocks.items():
        starts = [i for i, l in enumerate(lines) if l.strip().startswith("///") and first_words in l]
        if len(starts) != 1:
            fail(f"{path}: expected one doc block starting {first_words!r}, found {len(starts)}")
        i = starts[0]
        while i < len(lines) and lines[i].strip().startswith("///"):
            i += 1
        while i < len(lines) and lines[i].strip().startswith("#["):
            i += 1
        found = lines[i].strip()[:60] if i < len(lines) else "<end of file>"
        if not found.startswith(item):
            fail(f"{path}: the doc block {first_words!r} documents {found!r}, not {item!r}")
    if code_lines(text) != code_lines(repo_show(PARENT, path).decode("utf-8")):
        fail(f"{path}: a line other than a doc comment changed")
ok("every displaced doc block documents its own item again")
