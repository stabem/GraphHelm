"""Keel agent lock (#2987, #2989): a PreToolUse hook that refuses three shell forms for Keel agents, and a brief that starts a Keel agent with a bad identity line.

The agent files say "no heredoc", "no sleep", "never git stash" and agents broke each of them. This
is the machine side of those sentences. It reads the host's PreToolUse JSON on stdin; the caller is
`agent_type` (absent in a main session, `<plugin>:<agent>` in a plugin subagent; observed on Claude
Code 2.1.323). A caller that is not one of the names below is never touched.

The tool that starts an agent is `Agent`; its input carries `prompt` and
`subagent_type` (host hooks documentation, PreToolUse tool input). Starting a Keel agent
(`keel-author`, `keel-reviewer`, `keel-scout`, `keel-journey-qa`, bare or `keel:`/`graphhelm:` prefixed) is
refused from any caller unless some `Session:` line in the brief has a non-empty name that does not begin
with a Keel role name. Any other agent type is never refused.

A refusal is exit 0 with a `permissionDecision: deny` reply, never exit 2. Any failure of the hook
itself lets the command through and says so in a `systemMessage`. It spawns nothing and imports no
`subprocess`, so it opens no console window. Codex runs no hook from this file; that is unobserved.

The plugin ships this file and the graphhelm plugin ships a byte-identical copy (a plugin cannot read
a sibling plugin's files); the test beside it keeps the two equal.
"""
import json
import os
import re
import sys

KEEL_AGENTS = frozenset({
    "keel:keel-author", "keel:keel-reviewer", "keel:keel-scout",
    "graphhelm:keel-author", "graphhelm:keel-reviewer",
})
SHELL_TOOLS = ("Bash", "PowerShell")
AGENT_TOOLS = ("Agent",)
ROLE_NAMES = ("keel-author", "keel-reviewer", "keel-scout", "keel-journey-qa")

MESSAGES = {
    "heredoc": ("Keel rule: no heredoc in a shell command. Write the text to a file with the Write tool "
                "and pass its path (git commit -F <file>, gh pr create --body-file <file>)."),
    "sleep": ("Keel rule: no sleep. Wait on the command itself: run it in the background and read its "
              "result when it ends, or give the command a timeout; never poll with a delay."),
    "session": ("Keel rule: the brief to a Keel agent needs an identity line `Session: <session name> [ref] "
                "(keel-author subagent)`. The session name comes first and is never a Keel role name; the role "
                "goes in parentheses after it. Read the session name from the host, then brief again."),
    "stash": ("Keel rule: never git stash (the stash list is shared across worktrees). Commit the work "
              "on its own branch, or copy it to a file; only `git stash list` and `git stash show` are allowed."),
}

ROLE_PATTERN = "(?:(?:keel|graphhelm):)?(?:" + "|".join(ROLE_NAMES) + ")"
SESSION_LINE = re.compile(r"Session:[ \t]*([^\n`]*)")
TIMEOUT_OPTIONS_WITH_VALUE = frozenset({"-s", "--signal", "-k", "--kill-after"})
HEREDOC = re.compile(r"(?<!<)<<(?![<=])-?\s*[\w\\]")
SEPARATORS = re.compile(r"[;|&\n(){}`]+")
SKIPPED_WORDS = frozenset({"then", "do", "else", "elif", "if", "while", "until", "!", "time", "command",
                           "exec", "nohup", "sudo", "env", "nice", "xargs"})
GIT_OPTIONS_WITH_VALUE = frozenset({"-C", "-c", "--git-dir", "--work-tree", "--namespace",
                                    "--super-prefix", "--config-env", "--attr-source", "--exec-path"})


def is_keel_agent(agent_type):
    return isinstance(agent_type, str) and agent_type in KEEL_AGENTS


def code_text(command, tool):
    """The command with every quoted span replaced by one `Q`; command substitutions stay code.

    A quoted span cannot hide a rule, but `$(...)` inside a double quote is code again (that is where
    `git commit -m "$(cat <<'EOF' ...)"` lives), and a comment is no command.
    """
    escape = "`" if tool == "PowerShell" else "\\"
    out = []
    stack = ["code"]  # "code", "dq", "sq", "sub", "bt", "arith"
    depth = [0]
    i, n = 0, len(command)
    while i < n:
        ch = command[i]
        top = stack[-1]
        if top == "sq":
            if ch == "'":
                stack.pop()
            i += 1
            continue
        if ch == escape and top in ("code", "dq", "sub", "bt") and i + 1 < n:
            out.append(" " if top != "dq" else "")
            i += 2
            continue
        if top == "arith":
            if ch == "(":
                depth[-1] += 1
            elif ch == ")":
                if depth[-1] == 0 and command[i + 1:i + 2] == ")":
                    stack.pop()
                    depth.pop()
                    out.append("Q")
                    i += 2
                    continue
                depth[-1] = max(depth[-1] - 1, 0)
            i += 1
            continue
        if top == "dq":
            if ch == '"':
                stack.pop()
            elif ch == "$" and command[i + 1:i + 2] == "(" and command[i + 2:i + 3] != "(":
                stack.append("sub")
                depth.append(0)
                out.append(" ")
                i += 2
                continue
            elif ch == "`" and tool != "PowerShell":
                stack.append("bt")
                out.append(" ")
            i += 1
            continue
        # code-like: "code", "sub", "bt"
        if ch == "#" and (not out or out[-1] in " \t\n;|&(") and command[i - 1:i] != "$":
            while i < n and command[i] != "\n":
                i += 1
            continue
        if ch == "'":
            stack.append("sq")
            out.append("Q")
        elif ch == '"':
            stack.append("dq")
            out.append("Q")
        elif ch == "$" and command[i + 1:i + 3] == "((":
            stack.append("arith")
            depth.append(0)
            i += 3
            continue
        elif ch == "$" and command[i + 1:i + 2] == "(":
            stack.append("sub")
            depth.append(0)
            out.append(" ")
            i += 2
            continue
        elif ch == "`" and tool != "PowerShell":
            if top == "bt":
                stack.pop()
            else:
                stack.append("bt")
            out.append(" ")
        elif top == "sub" and ch == "(":
            depth[-1] += 1
            out.append(ch)
        elif top == "sub" and ch == ")":
            if depth[-1] == 0:
                stack.pop()
                depth.pop()
                out.append(" ")
            else:
                depth[-1] -= 1
                out.append(ch)
        else:
            out.append(ch)
        i += 1
    return "".join(out)


def _first_words(segment):
    words = segment.split()
    while words:
        if words[0] in SKIPPED_WORDS or re.fullmatch(r"[A-Za-z_]\w*=\S*", words[0]):
            words.pop(0)
        elif _base(words[0]) in ("timeout", "timeout.exe"):
            words.pop(0)
            while words and words[0].startswith("-"):
                del words[:2 if words[0] in TIMEOUT_OPTIONS_WITH_VALUE else 1]
            if words:
                words.pop(0)  # the duration
        else:
            break
    return words


def _base(word):
    return re.split(r"[\\/]", word)[-1].lower()


def _git_subcommand(args):
    i = 0
    while i < len(args) and args[i].startswith("-"):
        i += 2 if args[i] in GIT_OPTIONS_WITH_VALUE else 1
    if i >= len(args):
        return None, []
    return args[i], args[i + 1:]


def refusal(command, tool="Bash"):
    """(rule, message) for a refused command, else None. Pure; the table test drives it."""
    code = code_text(command, tool)
    if tool == "Bash" and HEREDOC.search(code):
        return "heredoc", MESSAGES["heredoc"]
    for segment in SEPARATORS.split(code):
        words = _first_words(segment)
        if not words:
            continue
        name = _base(words[0])
        if name in ("start-sleep", "start-sleep.exe") or (name in ("sleep", "sleep.exe") and len(words) > 1):
            return "sleep", MESSAGES["sleep"]
        if name in ("git", "git.exe"):
            sub, rest = _git_subcommand(words[1:])
            if sub == "stash" and not (rest and rest[0] in ("list", "show")):
                return "stash", MESSAGES["stash"]
    return None


def _agent_refusal(tool_input):
    """(rule, message) when this Agent call starts a Keel agent with a bad identity line, else None."""
    if not isinstance(tool_input, dict):
        return None
    agent = tool_input.get("subagent_type")
    if not isinstance(agent, str) or not re.fullmatch(ROLE_PATTERN, agent.strip(), re.IGNORECASE):
        return None
    prompt = tool_input.get("prompt")
    for found in SESSION_LINE.finditer(prompt if isinstance(prompt, str) else ""):
        name = re.sub(r"^[\s<*\"'(\[]+", "", found.group(1))
        if name and not re.match(ROLE_PATTERN, name, re.IGNORECASE):
            return None  # one good line is enough: a brief may quote the wrong form before the real line
    return "session", MESSAGES["session"]


def decide(raw):
    """The JSON line to print for this stdin (bytes), or None to say nothing. Never raises."""
    try:
        event = json.loads(raw.decode("utf-8", errors="replace"))
        if not isinstance(event, dict):
            raise ValueError("input is not an object")
        tool_input = event.get("tool_input")
        if event.get("tool_name") in AGENT_TOOLS:
            found = _agent_refusal(tool_input)
        elif is_keel_agent(event.get("agent_type")) and event.get("tool_name") in SHELL_TOOLS:
            command = tool_input.get("command") if isinstance(tool_input, dict) else None
            found = refusal(command, event["tool_name"]) if isinstance(command, str) else None
        else:
            return None
        if found is None:
            return None
        return json.dumps({"hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": found[1],
        }})
    except Exception as error:  # fail open, and say so
        return json.dumps({"systemMessage": "keel agent lock skipped: " + type(error).__name__})


def main():
    reply = decide(sys.stdin.buffer.read())
    if reply is not None:
        sys.stdout.write(reply + "\n")
    return 0


if __name__ == "__main__":
    try:
        code = main()
    except BaseException:  # noqa: BLE001 - nothing here may turn into exit 2
        sys.stdout.write('{"systemMessage": "keel agent lock skipped: hook failed"}\n')
        code = 0
    sys.stdout.flush()
    os._exit(code)
