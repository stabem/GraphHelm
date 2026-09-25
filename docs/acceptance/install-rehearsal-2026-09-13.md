# Clean-host install rehearsal — 2026-09-13

The Runtime half of [`install/VPS_REHEARSAL.md`](../../install/VPS_REHEARSAL.md) §4, run on a
genuinely clean host: a fresh `ubuntu:24.04` Docker container (Docker Desktop on a Windows 11
host), no Rust, no `graphhelm`, no systemd. This is the first recorded run of that document;
before it, the rehearsal was defined and referenced by nothing (#1062).

| | |
|---|---|
| Commit under test | `2ccbe1f6b2f3b8d82079ec95cbf53da05a0f4956` (branch `issue-1062-graphhelm-init`) |
| Host | `ubuntu:24.04` image, `Ubuntu 24.04.4 LTS`, `x86_64`, `nproc` = 8, container started with `sleep infinity` |
| Source | `git archive HEAD` of the commit above, extracted to `/src` (a plain container has no clone) |
| Started / ended (from the transcript) | `2026-09-13T21:37:16Z` / `2026-09-13T21:41:09Z` |
| Result | **pass** — every probe in sections 4–7 returned its expected value |
| Secrets | the token and the key appear as SHA-256 digests only; section 9 is the transcript's own sweep for any 64-hex string |
| Cost | none — no VPS, no paid infrastructure |

Every line below is the transcript as the container wrote it (`/rehearsal.log`), cut at the
section markers; the only edits are the three `[... elided ...]` markers replacing `apt-get` and
`rustup` progress output, and `<events.token>` / `<events.token value>` in the echoed commands,
which the script itself printed instead of the value. The script is reproduced at the end.

## What was rehearsed, and what was not

**Rehearsed:** the clean-host build from source with the pinned toolchain, `graphhelm init` as a
non-root user, its idempotence, `graphhelm serve` from the paths `init` printed (with the keyring
pre-flight of `2ccbe1f6` in force), the `/health` → `401` → `404` probe sequence of §3/§4, and the
first fixture-driven execution reaching the Runtime.

**Not rehearsed** (a plain container cannot): `install/install.sh` itself past its first guard —
it refuses without `systemctl`, by design (section 2) — so the systemd unit, its restart,
`journalctl`, the `graphhelm:graphhelm 600 /var/lib/graphhelm/events.token` ownership, and the
second-run token-preservation check of §4 were **not** exercised. §3 (the Docker Compose path)
needs a Docker daemon inside the host and was not run here either. Those remain for a run on a
real VPS or a systemd-enabled container. The Studio (Node) was not started in the container.

## Transcript

### 1. Record the clean host

```

$ date -u +%Y-%m-%dT%H:%M:%SZ   (start)
2026-09-13T21:37:16Z

$ nproc
8

$ cat /etc/os-release
PRETTY_NAME="Ubuntu 24.04.4 LTS"
NAME="Ubuntu"
VERSION_ID="24.04"
VERSION="24.04.4 LTS (Noble Numbat)"
VERSION_CODENAME=noble
ID=ubuntu
ID_LIKE=debian
HOME_URL="https://www.ubuntu.com/"
SUPPORT_URL="https://help.ubuntu.com/"
BUG_REPORT_URL="https://bugs.launchpad.net/ubuntu/"
PRIVACY_POLICY_URL="https://www.ubuntu.com/legal/terms-and-policies/privacy-policy"
UBUNTU_CODENAME=noble
LOGO=ubuntu-logo
[exit 0]

$ uname -m
x86_64
[exit 0]

$ id
uid=0(root) gid=0(root) groups=0(root)
[exit 0]

$ cat /src/REHEARSAL_COMMIT
2ccbe1f6b2f3b8d82079ec95cbf53da05a0f4956

$ command -v systemctl || echo "systemctl: not found"
systemctl: not found

$ command -v cargo || echo "cargo: not found"
cargo: not found

$ command -v graphhelm || echo "graphhelm: not found"
graphhelm: not found
```

### 2. The native installer, as written (`VPS_REHEARSAL.md` §4)

```

$ ./install/install.sh
graphhelm install: systemd is required
[exit 1]
installer exit status: 1
```

Expected on a plain container: the installer's own guard (`command -v systemctl || fail
"systemd is required"`, `install/install.sh:39`) refuses before touching anything. Recorded
rather than worked around; everything below is the container-level equivalent.

### 3. Build the binary from source in the clean container

```

$ apt-get update -qq
[... apt output elided ...]
[exit 0]

$ apt-get install --yes --no-install-recommends -qq build-essential ca-certificates curl git pkg-config
[... apt output elided ...]
[exit 0]

$ useradd --create-home --shell /bin/bash rehearsal
[exit 0]

$ cp -r /src /home/rehearsal/GraphHelm
[exit 0]

$ chown -R rehearsal:rehearsal /home/rehearsal/GraphHelm
[exit 0]

$ runuser --user rehearsal -- bash -lc 'curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.97.1'
[... rustup output elided ...]
[exit 0]

$ cargo +1.97.1 --version
cargo 1.97.1 (c980f4866 2026-06-30)

$ cargo +1.97.1 install --locked --path apps/cli   (in /home/rehearsal/GraphHelm)
   Compiling graphhelm-cli v0.1.0 (/home/rehearsal/GraphHelm/apps/cli)
    Finished `release` profile [optimized] target(s) in 3m 06s
  Installing /home/rehearsal/.cargo/bin/graphhelm
   Installed package `graphhelm-cli v0.1.0 (/home/rehearsal/GraphHelm/apps/cli)` (executable `graphhelm`)
[exit 0]
build wall time: 186 s

$ graphhelm --version
graphhelm 0.1.0
```

The build is `-p graphhelm-cli` only (`cargo install --path apps/cli`), which is what
`GETTING_STARTED.md` §1 tells a stranger to run; the whole workspace was not built in the
container. The wall time is the one measured in this run (`build wall time: 186 s`).

### 4. `graphhelm init` in a fresh project, as the non-root user

```

$ graphhelm init --project /home/rehearsal/project --harness claude-code --pretty
{
  "ok": true,
  "command": "init",
  "data": {
    "bind": "127.0.0.1:8791",
    "events": {
      "path": ".graphhelm/events",
      "state": "created"
    },
    "gitignore": {
      "path": ".gitignore",
      "state": "created"
    },
    "harnesses": [
      {
        "harness": "claude-code",
        "note": "Claude Code reads this file from the project root; restart the session to pick it up.",
        "path": ".mcp.json",
        "state": "created"
      }
    ],
    "key": {
      "environment": "GRAPHHELM_EVENTS_KEY",
      "path": ".graphhelm/serve.key",
      "state": "created"
    },
    "keyring": {
      "keyId": "studio",
      "path": ".graphhelm/keyring",
      "state": "created"
    },
    "next": [
      {
        "bash": "export GRAPHHELM_EVENTS_KEY=\"$(cat \"/home/rehearsal/project/.graphhelm/serve.key\")\"",
        "powershell": "$env:GRAPHHELM_EVENTS_KEY = (Get-Content -Raw '/home/rehearsal/project/.graphhelm/serve.key').Trim()",
        "step": "export the sealing key from serve.key (the Runtime needs it in the environment; it is never passed as a flag)"
      },
      {
        "bash": "graphhelm serve --events \"/home/rehearsal/project/.graphhelm/events\" --bind 127.0.0.1:8791 --keyring \"/home/rehearsal/project/.graphhelm/keyring\" --key-id studio",
        "powershell": "graphhelm serve --events '/home/rehearsal/project/.graphhelm/events' --bind 127.0.0.1:8791 --keyring '/home/rehearsal/project/.graphhelm/keyring' --key-id studio",
        "step": "start the Runtime on loopback (it refuses at start if the key or the key id is wrong)"
      },
      {
        "bash": "npm --prefix apps/studio ci && GRAPHHELM_EVENTS=\"/home/rehearsal/project/.graphhelm/events\" GRAPHHELM_RUNTIME_URL=\"http://127.0.0.1:8791\" npm --prefix apps/studio run dev",
        "powershell": "powershell -File apps/studio/tools/studio-up.ps1 -Events '/home/rehearsal/project/.graphhelm/events' -Bind 127.0.0.1:8791 -Keyring '/home/rehearsal/project/.graphhelm/keyring' -KeyId studio -GraphHelm graphhelm",
        "step": "start the Studio, from your GraphHelm clone (Node 22+ and npm are needed only for this step)"
      },
      {
        "bash": "echo '{\"nodeOutcomes\":{\"implementation\":\"failure\"}}' > \"/home/rehearsal/project/.graphhelm/fixtures.json\" && graphhelm execution start --file examples/graphs/manual-override-deploy.yaml --events \"/home/rehearsal/project/.graphhelm/events\" --fixtures \"/home/rehearsal/project/.graphhelm/fixtures.json\" --mode supervised --execution demo",
        "powershell": "Set-Content -Path '/home/rehearsal/project/.graphhelm/fixtures.json' -Value '{\"nodeOutcomes\":{\"implementation\":\"failure\"}}'; graphhelm execution start --file examples/graphs/manual-override-deploy.yaml --events '/home/rehearsal/project/.graphhelm/events' --fixtures '/home/rehearsal/project/.graphhelm/fixtures.json' --mode supervised --execution demo",
        "step": "start the first execution, offline, from your GraphHelm clone (the fixture stands in for a model)"
      }
    ],
    "project": "/home/rehearsal/project",
    "root": ".graphhelm",
    "token": {
      "path": ".graphhelm/events.token",
      "state": "created"
    }
  },
  "diagnostics": []
}
[exit 0]

$ stat --format '%U:%G %a %n' ~/project/.graphhelm/events.token ~/project/.graphhelm/serve.key
rehearsal:rehearsal 600 /home/rehearsal/project/.graphhelm/events.token
rehearsal:rehearsal 600 /home/rehearsal/project/.graphhelm/serve.key

$ cat ~/project/.gitignore
# GraphHelm Runtime working directory: bearer token, event store, sealing key.
.graphhelm/

$ cat ~/project/.mcp.json
{
  "mcpServers": {
    "graphhelm": {
      "args": [
        "mcp",
        "--url",
        "http://127.0.0.1:8791",
        "--token-file",
        "/home/rehearsal/project/.graphhelm/events.token",
        "--actor",
        "agent-chat"
      ],
      "command": "/home/rehearsal/.cargo/bin/graphhelm"
    }
  }
}

$ git -C ~/project check-ignore -v .graphhelm/events.token
.gitignore:2:.graphhelm/	.graphhelm/events.token

$ TOKEN_SHA_1=$(sha256sum ~/project/.graphhelm/events.token | cut -d' ' -f1)
token_sha256=43047dd1cf53a89b8fbeb9afb57a830ca0b08dac0f46bee6b36ae1ba5f0e18dd
key_sha256=67a1b606ec6d11958b102cd70e668e77aefd563da7f2eea8be78231b5a6f7e79
```

`--harness claude-code` was passed explicitly because the container has neither `~/.claude` nor
`~/.codex` and detection would have registered nothing. Note the shape at this head: paths in
`data` are project-relative, the `next` strings carry the absolute paths, and the registration's
`command` is the binary that ran `init` (`/home/rehearsal/.cargo/bin/graphhelm`).

### 5. A second `init` keeps every secret

```

$ graphhelm init --project /home/rehearsal/project --harness claude-code
{"ok":true,"command":"init","data":{"bind":"127.0.0.1:8791","events":{"path":".graphhelm/events","state":"existing"},"gitignore":{"path":".gitignore","state":"existing"},"harnesses":[{"harness":"claude-code","note":"Claude Code reads this file from the project root; restart the session to pick it up.","path":".mcp.json","state":"existing"}],"key":{"environment":"GRAPHHELM_EVENTS_KEY","path":".graphhelm/serve.key","state":"existing"},"keyring":{"keyId":"studio","path":".graphhelm/keyring","state":"existing"},"next":[{"bash":"export GRAPHHELM_EVENTS_KEY=\"$(cat \"/home/rehearsal/project/.graphhelm/serve.key\")\"","powershell":"$env:GRAPHHELM_EVENTS_KEY = (Get-Content -Raw '/home/rehearsal/project/.graphhelm/serve.key').Trim()","step":"export the sealing key from serve.key (the Runtime needs it in the environment; it is never passed as a flag)"},{"bash":"graphhelm serve --events \"/home/rehearsal/project/.graphhelm/events\" --bind 127.0.0.1:8791 --keyring \"/home/rehearsal/project/.graphhelm/keyring\" --key-id studio","powershell":"graphhelm serve --events '/home/rehearsal/project/.graphhelm/events' --bind 127.0.0.1:8791 --keyring '/home/rehearsal/project/.graphhelm/keyring' --key-id studio","step":"start the Runtime on loopback (it refuses at start if the key or the key id is wrong)"},{"bash":"npm --prefix apps/studio ci && GRAPHHELM_EVENTS=\"/home/rehearsal/project/.graphhelm/events\" GRAPHHELM_RUNTIME_URL=\"http://127.0.0.1:8791\" npm --prefix apps/studio run dev","powershell":"powershell -File apps/studio/tools/studio-up.ps1 -Events '/home/rehearsal/project/.graphhelm/events' -Bind 127.0.0.1:8791 -Keyring '/home/rehearsal/project/.graphhelm/keyring' -KeyId studio -GraphHelm graphhelm","step":"start the Studio, from your GraphHelm clone (Node 22+ and npm are needed only for this step)"},{"bash":"echo '{\"nodeOutcomes\":{\"implementation\":\"failure\"}}' > \"/home/rehearsal/project/.graphhelm/fixtures.json\" && graphhelm execution start --file examples/graphs/manual-override-deploy.yaml --events \"/home/rehearsal/project/.graphhelm/events\" --fixtures \"/home/rehearsal/project/.graphhelm/fixtures.json\" --mode supervised --execution demo","powershell":"Set-Content -Path '/home/rehearsal/project/.graphhelm/fixtures.json' -Value '{\"nodeOutcomes\":{\"implementation\":\"failure\"}}'; graphhelm execution start --file examples/graphs/manual-override-deploy.yaml --events '/home/rehearsal/project/.graphhelm/events' --fixtures '/home/rehearsal/project/.graphhelm/fixtures.json' --mode supervised --execution demo","step":"start the first execution, offline, from your GraphHelm clone (the fixture stands in for a model)"}],"project":"/home/rehearsal/project","root":".graphhelm","token":{"path":".graphhelm/events.token","state":"existing"}},"diagnostics":[]}
[exit 0]

$ test "$TOKEN_SHA_1" = "$TOKEN_SHA_2" && test "$KEY_SHA_1" = "$KEY_SHA_2" && echo "token and key unchanged"
token and key unchanged
[exit 0]
```

### 6. `graphhelm serve` with the printed command; health, 401, 404 probes

```

$ export GRAPHHELM_EVENTS_KEY="$(cat ~/project/.graphhelm/serve.key)"; graphhelm serve --events ~/project/.graphhelm/events --bind 127.0.0.1:8791 --keyring ~/project/.graphhelm/keyring --key-id studio > ~/serve.log 2>&1 &

$ cat ~/serve.log   (the serve.started line)
{"ok":true,"command":"serve.started","data":{"address":"127.0.0.1:8791"},"diagnostics":[]}

$ curl --silent --show-error --fail http://127.0.0.1:8791/health
{"ok":true,"command":"serve.health","data":{},"diagnostics":[]}
[exit 0]

$ curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:8791/v1/executions   (no token)
401

$ curl -s -o /dev/null -w '%{http_code}' -H 'Authorization: Bearer <events.token>' http://127.0.0.1:8791/does-not-exist
404

$ curl -s -H 'Authorization: Bearer <events.token>' 'http://127.0.0.1:8791/v1/executions?limit=5'
{"ok":true,"command":"execution.list","data":{"executions":[],"hasMore":false,"nextCursor":null},"diagnostics":[]}
```

These are §3/§4's probes of `VPS_REHEARSAL.md` at the `init` port: `/health` carries `"ok":true`
and `"command":"serve.health"`; a bare request is `401`; the persisted token reaches the router
and an unknown path is `404`.

### 7. The first execution against the same store, and the Runtime sees it

```

$ echo '{"nodeOutcomes":{"implementation":"failure"}}' > ~/project/.graphhelm/fixtures.json

$ graphhelm execution start --file ~/GraphHelm/examples/graphs/manual-override-deploy.yaml --events ~/project/.graphhelm/events --fixtures ~/project/.graphhelm/fixtures.json --mode supervised --execution demo
{"ok":true,"command":"execution.start","data":{"acceptedMutations":0,"attention":"needs_you","attentionReasons":[{"kind":"blocked_node","node":"implementation"}],"customs":{"clearances":{},"nodes":{},"quarantinedNodes":[]},"executionId":"demo","lastEventAt":"2026-09-13T21:41:09.072164303+00:00","mode":"supervised","nodeLastEventAt":{"deploy":"2026-09-13T21:41:08.911462652+00:00","implementation":"2026-09-13T21:41:09.072164303+00:00"},"nodeStateCounts":{"blocked":1,"cancelled":0,"draft":0,"failed":0,"ghost":0,"invalidated":0,"linting":0,"paused":0,"queued":0,"ready":1,"running":0,"skipped":0,"succeeded":0,"waiting_capacity":0,"waiting_input":0,"waived":0},"nodeStates":{"deploy":"ready","implementation":"blocked"},"signalsRecorded":0,"silenceUnevaluated":[],"startedAt":"2026-09-13T21:41:08.894542034+00:00","status":"running","untriagedInterruptions":[]},"diagnostics":[{"code":"GHG102_UNBOUNDED_CUSTOMS","severity":"warning","message":"node can park for input but declares no customs budgets, so no stage of it can ever go overdue","path":"/spec/nodes/deploy/completion/customs","source":"manual-override-deploy.yaml"},{"code":"GHG101_DEFAULT_TIMEOUT","severity":"warning","message":"executable node relies on the runtime default timeout","path":"/spec/nodes/deploy/timeoutSeconds","source":"manual-override-deploy.yaml"},{"code":"GHG102_UNBOUNDED_CUSTOMS","severity":"warning","message":"node can park for input but declares no customs budgets, so no stage of it can ever go overdue","path":"/spec/nodes/implementation/completion/customs","source":"manual-override-deploy.yaml"},{"code":"GHG101_DEFAULT_TIMEOUT","severity":"warning","message":"executable node relies on the runtime default timeout","path":"/spec/nodes/implementation/timeoutSeconds","source":"manual-override-deploy.yaml"}]}
[exit 0]

$ curl -s -H 'Authorization: Bearer <events.token>' http://127.0.0.1:8791/v1/executions/demo   (the status route carries the attention verdict)
{"ok":true,"command":"execution.status","data":{"acceptedMutations":0,"attention":"needs_you","attentionReasons":[{"kind":"blocked_node","node":"implementation"}],"customs":{"clearances":{},"nodes":{},"quarantinedNodes":[]},"executionId":"demo","headSequence":13,"lastEventAt":"2026-09-13T21:41:09.072164303+00:00","mode":"supervised","nodeLastEventAt":{"deploy":"2026-09-13T21:41:08.911462652+00:00","implementation":"2026-09-13T21:41:09.072164303+00:00"},"nodeStateCounts":{"blocked":1,"cancelled":0,"draft":0,"failed":0,"ghost":0,"invalidated":0,"linting":0,"paused":0,"queued":0,"ready":1,"running":0,"skipped":0,"succeeded":0,"waiting_capacity":0,"waiting_input":0,"waived":0},"nodeStates":{"deploy":"ready","implementation":"blocked"},"signalsRecorded":0,"silenceUnevaluated":[],"startedAt":"2026-09-13T21:41:08.894542034+00:00","status":"running","untriagedInterruptions":[]},"diagnostics":[]}

$ stat --format '%U:%G %a %n' ~/project/.graphhelm/keyring
rehearsal:rehearsal 700 /home/rehearsal/project/.graphhelm/keyring
```

The CLI wrote the run into the store `init` created while `serve` was holding it, and the
Runtime answered the same `needs_you` / `blocked_node: implementation` verdict over HTTP — the
handoff `GETTING_STARTED.md` §5 relies on.

### 8. Final acceptance record

```
commit=2ccbe1f6b2f3b8d82079ec95cbf53da05a0f4956
installer_exit=1 (systemd absent in a plain container)
build_exit=0
health=200
token_sha256=43047dd1cf53a89b8fbeb9afb57a830ca0b08dac0f46bee6b36ae1ba5f0e18dd
key_sha256=67a1b606ec6d11958b102cd70e668e77aefd563da7f2eea8be78231b5a6f7e79

$ grep -c '<events.token value>' ~/serve.log  (the token must not be in the serve log)
0
occurrences above (0 expected)

$ date -u +%Y-%m-%dT%H:%M:%SZ   (end)
2026-09-13T21:41:09Z
```

### 9. The transcript's own sweep for 64-hex strings

```

$ grep -oE '[0-9a-f]{64}' /rehearsal.log | sort | uniq -c
      2 43047dd1cf53a89b8fbeb9afb57a830ca0b08dac0f46bee6b36ae1ba5f0e18dd
      2 67a1b606ec6d11958b102cd70e668e77aefd563da7f2eea8be78231b5a6f7e79
```

Two distinct strings, each the SHA-256 digest printed in sections 4 and 8; the raw token and key
never entered the log.

## Findings, across the runs of this script

1. **`init` was refused on Ubuntu and passed on Windows** (run at `26a9ff8e`, the commit that
   introduced `graphhelm init`). The keyring step failed with `GHCLI027_INIT_REFUSED` at
   `/keyring`: `SealedKeyProvider::create` requires the keyring directory to be owner-only
   (`0700`, `validate_secure_directory`), and `init` had created it with `create_dir_all` under
   the default umask (`0755`). Windows performs no such check, so the CLI suite was green on the
   development machine. Fixed in `2641fe13` (`init` sets `0700` on every run and the refusal
   names the rule); the Unix branch of `apps/cli/tests/init_cli.rs` asserts the mode, and section
   4 above shows `700`. This is exactly the class of defect the issue said an unrun rehearsal
   hides.
2. **One relaunch tested the wrong bytes.** A relaunch after that fix shipped the previous
   archive by mistake (the tar was written into the worktree instead of the staging directory,
   and the stale copy was copied in). Caught by comparing the refusal text against the fixed
   source, and since then every launch greps the container's copy of `init.rs` for the change
   under test before the script starts.
3. **`install/install.sh` cannot be rehearsed in a plain container** — by its own design
   (section 2). The systemd half of `VPS_REHEARSAL.md` §4 stays unrun.
4. The `/v1/executions/{id}` status route is where the attention verdict lives; there is no
   `/attention` sub-route (an early version of this script asked for one and got
   `GHCLI008_SERVE_NOT_FOUND`; section 7 uses the status route).
5. Earlier runs of this script at `26a9ff8e`, `2641fe13` and the two intermediate relaunches are
   not reproduced here; their logs were overwritten by the next launch and only the outcomes
   above were kept. This record is the run at `2ccbe1f6b2f3b8d82079ec95cbf53da05a0f4956` only.

## The script

Run as root inside the container, with the archive at `/src`; the log is `/rehearsal.log`.
Reproduced verbatim.

```bash
#!/usr/bin/env bash
# Clean-host rehearsal for #1062, run as root inside a fresh ubuntu:24.04 container.
# The source tree is at /src (git archive of the commit named in /src/REHEARSAL_COMMIT).
# Every command is echoed before it runs and its exit status after; tokens are hashed.
set -uo pipefail
export DEBIAN_FRONTEND=noninteractive

show() { printf '\n$ %s\n' "$*"; }
run() { show "$*"; "$@"; local rc=$?; printf '[exit %s]\n' "$rc"; return $rc; }

echo "=== 1. Record the clean host ==="
show 'date -u +%Y-%m-%dT%H:%M:%SZ   (start)'; date -u +%Y-%m-%dT%H:%M:%SZ
show 'nproc'; nproc
run cat /etc/os-release
run uname -m
run id
show 'cat /src/REHEARSAL_COMMIT'; cat /src/REHEARSAL_COMMIT
show 'command -v systemctl || echo "systemctl: not found"'; command -v systemctl || echo "systemctl: not found"
show 'command -v cargo || echo "cargo: not found"'; command -v cargo || echo "cargo: not found"
show 'command -v graphhelm || echo "graphhelm: not found"'; command -v graphhelm || echo "graphhelm: not found"

echo
echo "=== 2. The native installer, as written (install/VPS_REHEARSAL.md section 4) ==="
cd /src || exit 1
run ./install/install.sh
INSTALLER_RC=$?
echo "installer exit status: ${INSTALLER_RC}"

echo
echo "=== 3. Container-level equivalent: build the binary from source in the clean container ==="
run apt-get update -qq
run apt-get install --yes --no-install-recommends -qq build-essential ca-certificates curl git pkg-config
run useradd --create-home --shell /bin/bash rehearsal
run cp -r /src /home/rehearsal/GraphHelm
run chown -R rehearsal:rehearsal /home/rehearsal/GraphHelm

as_user() { runuser --user rehearsal -- bash -lc "$*"; }
show "runuser --user rehearsal -- bash -lc 'curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.97.1'"
as_user 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.97.1 2>&1 | tail -5'
printf '[exit %s]\n' $?
show "cargo +1.97.1 --version"
as_user 'cargo +1.97.1 --version'

BUILD_START=$(date +%s)
show "cargo +1.97.1 install --locked --path apps/cli   (in /home/rehearsal/GraphHelm)"
as_user 'cd ~/GraphHelm && cargo +1.97.1 install --locked --path apps/cli 2>&1 | tail -4'
BUILD_RC=$?
printf '[exit %s]\n' "$BUILD_RC"
BUILD_END=$(date +%s)
echo "build wall time: $((BUILD_END - BUILD_START)) s"
show "graphhelm --version"
as_user 'graphhelm --version'

echo
echo "=== 4. graphhelm init in a fresh project, as the non-root user ==="
as_user 'mkdir -p ~/project && cd ~/project && git init -q . && echo "# demo" > README.md'
show "graphhelm init --project /home/rehearsal/project --harness claude-code --pretty"
as_user 'graphhelm init --project /home/rehearsal/project --harness claude-code --pretty'
printf '[exit %s]\n' $?
show "stat --format '%U:%G %a %n' ~/project/.graphhelm/events.token ~/project/.graphhelm/serve.key"
as_user 'stat --format "%U:%G %a %n" ~/project/.graphhelm/events.token ~/project/.graphhelm/serve.key'
show "cat ~/project/.gitignore"
as_user 'cat ~/project/.gitignore'
show "cat ~/project/.mcp.json"
as_user 'cat ~/project/.mcp.json'
show "git -C ~/project check-ignore -v .graphhelm/events.token"
as_user 'git -C ~/project check-ignore -v .graphhelm/events.token'
show "TOKEN_SHA_1=\$(sha256sum ~/project/.graphhelm/events.token | cut -d' ' -f1)"
TOKEN_SHA_1=$(as_user 'sha256sum ~/project/.graphhelm/events.token | cut -d" " -f1')
echo "token_sha256=${TOKEN_SHA_1}"
KEY_SHA_1=$(as_user 'sha256sum ~/project/.graphhelm/serve.key | cut -d" " -f1')
echo "key_sha256=${KEY_SHA_1}"

echo
echo "=== 5. A second init keeps every secret ==="
show "graphhelm init --project /home/rehearsal/project --harness claude-code"
as_user 'graphhelm init --project /home/rehearsal/project --harness claude-code'
printf '[exit %s]\n' $?
TOKEN_SHA_2=$(as_user 'sha256sum ~/project/.graphhelm/events.token | cut -d" " -f1')
KEY_SHA_2=$(as_user 'sha256sum ~/project/.graphhelm/serve.key | cut -d" " -f1')
show 'test "$TOKEN_SHA_1" = "$TOKEN_SHA_2" && test "$KEY_SHA_1" = "$KEY_SHA_2" && echo "token and key unchanged"'
test "$TOKEN_SHA_1" = "$TOKEN_SHA_2" && test "$KEY_SHA_1" = "$KEY_SHA_2" && echo "token and key unchanged"
printf '[exit %s]\n' $?

echo
echo "=== 6. graphhelm serve with the printed command; health, 401, 404 probes ==="
show 'export GRAPHHELM_EVENTS_KEY="$(cat ~/project/.graphhelm/serve.key)"; graphhelm serve --events ~/project/.graphhelm/events --bind 127.0.0.1:8791 --keyring ~/project/.graphhelm/keyring --key-id studio > ~/serve.log 2>&1 &'
as_user 'export GRAPHHELM_EVENTS_KEY="$(cat ~/project/.graphhelm/serve.key)"; nohup graphhelm serve --events ~/project/.graphhelm/events --bind 127.0.0.1:8791 --keyring ~/project/.graphhelm/keyring --key-id studio > ~/serve.log 2>&1 &'
for attempt in $(seq 1 30); do
  if curl --silent --show-error --fail --max-time 2 http://127.0.0.1:8791/health >/dev/null 2>&1; then break; fi
  sleep 1
done
show "cat ~/serve.log   (the serve.started line)"
as_user 'cat ~/serve.log'
show "curl --silent --show-error --fail http://127.0.0.1:8791/health"
curl --silent --show-error --fail http://127.0.0.1:8791/health; echo; printf '[exit %s]\n' $?
show "curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:8791/v1/executions   (no token)"
curl --silent --output /dev/null --write-out '%{http_code}\n' http://127.0.0.1:8791/v1/executions
show "curl -s -o /dev/null -w '%{http_code}' -H 'Authorization: Bearer <events.token>' http://127.0.0.1:8791/does-not-exist"
TOKEN_VALUE=$(as_user 'cat ~/project/.graphhelm/events.token')
curl --silent --output /dev/null --write-out '%{http_code}\n' --header "Authorization: Bearer ${TOKEN_VALUE}" http://127.0.0.1:8791/does-not-exist
show "curl -s -H 'Authorization: Bearer <events.token>' 'http://127.0.0.1:8791/v1/executions?limit=5'"
curl --silent --header "Authorization: Bearer ${TOKEN_VALUE}" 'http://127.0.0.1:8791/v1/executions?limit=5'; echo
unset TOKEN_VALUE

echo
echo "=== 7. The first execution against the same store, and the Runtime sees it ==="
show "echo '{\"nodeOutcomes\":{\"implementation\":\"failure\"}}' > ~/project/.graphhelm/fixtures.json"
as_user 'echo "{\"nodeOutcomes\":{\"implementation\":\"failure\"}}" > ~/project/.graphhelm/fixtures.json'
show "graphhelm execution start --file ~/GraphHelm/examples/graphs/manual-override-deploy.yaml --events ~/project/.graphhelm/events --fixtures ~/project/.graphhelm/fixtures.json --mode supervised --execution demo"
as_user 'graphhelm execution start --file ~/GraphHelm/examples/graphs/manual-override-deploy.yaml --events ~/project/.graphhelm/events --fixtures ~/project/.graphhelm/fixtures.json --mode supervised --execution demo'
printf '[exit %s]\n' $?
show "curl -s -H 'Authorization: Bearer <events.token>' http://127.0.0.1:8791/v1/executions/demo   (the status route carries the attention verdict)"
TOKEN_VALUE=$(as_user 'cat ~/project/.graphhelm/events.token')
curl --silent --header "Authorization: Bearer ${TOKEN_VALUE}" http://127.0.0.1:8791/v1/executions/demo; echo
unset TOKEN_VALUE
show "stat --format '%U:%G %a %n' ~/project/.graphhelm/keyring"
as_user 'stat --format "%U:%G %a %n" ~/project/.graphhelm/keyring'

echo
echo "=== 8. Final acceptance record ==="
printf 'commit=%s\n' "$(cat /src/REHEARSAL_COMMIT)"
printf 'installer_exit=%s (systemd absent in a plain container)\n' "${INSTALLER_RC}"
printf 'build_exit=%s\n' "${BUILD_RC}"
printf 'health=%s\n' "$(curl --silent --output /dev/null --write-out '%{http_code}' http://127.0.0.1:8791/health)"
printf 'token_sha256=%s\n' "${TOKEN_SHA_2}"
printf 'key_sha256=%s\n' "${KEY_SHA_2}"
show "grep -c '<events.token value>' ~/serve.log  (the token must not be in the serve log)"
as_user 'grep -c "$(cat ~/project/.graphhelm/events.token)" ~/serve.log; echo "occurrences above (0 expected)"'
pkill -f 'graphhelm serve' || true
show 'date -u +%Y-%m-%dT%H:%M:%SZ   (end)'; date -u +%Y-%m-%dT%H:%M:%SZ
echo "=== 9. Sweep this transcript for any 64-hex string (only the two digests may appear) ==="
show "grep -oE '[0-9a-f]{64}' /rehearsal.log | sort | uniq -c"
grep -oE '[0-9a-f]{64}' /rehearsal.log | sort | uniq -c
echo "=== rehearsal end ==="
```
