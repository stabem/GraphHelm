"""PreToolUse hook: deny a shell call that posts a GitHub comment without a filled identity line.

Covers `gh pr comment`, `gh issue comment` and `gh pr review` in a Bash or PowerShell call. The body
is read from `--body`/`-b` or from the readable file named by `--body-file`/`-F`. Whatever cannot be
determined (stdin, an editor, a command substitution, an unreadable file, an unparsable command) is
allowed: the hook never blocks on its own failure. No network, no child process.
"""

from __future__ import annotations

import json
import os
import re
import shlex
import sys

SHELL_TOOLS = ("Bash", "PowerShell")
READ_LIMIT = 4096
SEPARATOR_CHARS = ";&|(){}`"
SEPARATORS = set(SEPARATOR_CHARS)
# A word before `gh` that still leaves `gh` in command position (a keyword or a command wrapper).
LEADS_A_COMMAND = {"if", "then", "else", "elif", "while", "until", "do", "time", "sudo", "env", "exec", "nohup",
                   "xargs", "command", "builtin", "foreach", "for", "try", "catch", "finally", "switch",
                   "function", "begin", "process", "end", "not", "elseif", "in"}
ASSIGNMENT = re.compile(r"^[A-Za-z_]\w*=")
IDENTITY = re.compile(
    r"^Session: (?P<name>[^\[\]\n]*?\S) \[(?P<ref>[^\]\n]*\S)\](?: \([^)\n]*\))* · Head: "
    r"(?P<sha>[0-9A-Fa-f]{8,})(?![0-9A-Za-z_])")
UNKNOWN_START = ("$", "`", "@")
MESSAGE = ("This GitHub comment does not start with an identity line, so nobody can tell which session "
           "wrote it (many sessions post under one account). Make the first line of the body: "
           "`Session: <session name> [<ref>] · Head: <sha8>`, with your real session name, ref and the "
           "first 8 hex digits of your head commit, for example "
           "`Session: Ronda Keel [local_ab12cd34] · Head: 65c3b500`. Then post again.")


def filled(body: str) -> bool:
    for line in body.lstrip("﻿").splitlines():
        if line.strip():
            found = IDENTITY.match(line.strip().lstrip("﻿"))
            return bool(found) and not any(c in found["name"] + found["ref"] for c in "<>")
    return False


def _spaced(command: str) -> str:
    """The command with every unquoted newline and operator turned into a separator token."""
    out: list[str] = []
    quote = ""
    for char in command:
        if quote:
            quote = "" if char == quote else quote
        elif char in "'\"":
            quote = char
        elif char in "\r\n":
            char = " ; "
        elif char in SEPARATOR_CHARS:
            char = f" {char} "
        out.append(char)
    return "".join(out)


def _plain_argument(token: str) -> bool:
    """True only for an ordinary word of a simple command; punctuation, keywords and wrappers are not."""
    return (any(c.isalnum() for c in token) and token.lower() not in LEADS_A_COMMAND
            and not ASSIGNMENT.match(token))


def _tokens(command: str) -> list[str]:
    lexer = shlex.shlex(_spaced(command), posix=True)
    lexer.whitespace_split = True
    lexer.escape = ""  # a Windows path keeps its backslashes
    lexer.commenters = ""
    return list(lexer)


def _is_comment_command(words: list[str]) -> bool:
    return len(words) >= 3 and words[0] in ("gh", "gh.exe") and (
        (words[1] in ("pr", "issue") and words[2] == "comment") or (words[1] == "pr" and words[2] == "review"))


def _segments(tokens: list[str]) -> list[list[str]]:
    out: list[list[str]] = []
    for index, token in enumerate(tokens):
        starts_command = index == 0 or not _plain_argument(tokens[index - 1])
        if starts_command and token in ("gh", "gh.exe") and _is_comment_command(tokens[index:index + 3]):
            end = index + 1
            while end < len(tokens) and tokens[end] not in SEPARATORS:
                end += 1
            out.append(tokens[index:end])
    return out


def _body(words: list[str], cwd: str | None) -> str | None:
    """The body text this gh call posts, or None when it cannot be determined."""
    for index, word in enumerate(words):
        flag, eq, inline = word.partition("=")
        if flag in ("--body", "-b", "--body-file", "-F"):
            value = inline if eq else (words[index + 1] if index + 1 < len(words) else None)
            if value is None:
                return None
            if flag in ("--body", "-b"):
                return None if value.startswith(UNKNOWN_START) else value
            if value == "-" or value.startswith(UNKNOWN_START):
                return None
            path = value if os.path.isabs(value) or not cwd else os.path.join(cwd, value)
            try:
                with open(path, "rb") as handle:
                    return handle.read(READ_LIMIT).decode("utf-8", errors="replace")
            except OSError:
                return None
    return None


def decide(raw: bytes) -> str | None:
    """The JSON line to print for this stdin, or None to allow. Never raises."""
    try:
        event = json.loads(raw.decode("utf-8", errors="replace"))
        if not isinstance(event, dict) or event.get("tool_name") not in SHELL_TOOLS:
            return None
        tool_input = event.get("tool_input")
        command = tool_input.get("command") if isinstance(tool_input, dict) else None
        if not isinstance(command, str) or "gh" not in command:
            return None
        cwd = event.get("cwd") if isinstance(event.get("cwd"), str) else None
        for words in _segments(_tokens(command)):
            body = _body(words, cwd)
            if body is not None and not filled(body):
                return json.dumps({"hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": MESSAGE}})
        return None
    except Exception:  # noqa: BLE001 - deliberate: the hook never blocks on its own failure
        return None


def main() -> int:
    reply = decide(sys.stdin.buffer.read())
    if reply is not None:
        sys.stdout.write(reply + "\n")
    return 0


if __name__ == "__main__":
    try:
        code = main()
    except BaseException:  # noqa: BLE001 - nothing here may turn into a block
        code = 0
    sys.stdout.flush()
    os._exit(code)
