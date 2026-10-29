//! The non-browser observer (#927): a journey flow whose acts are `http` requests or `cli`
//! commands is played without a browser. Each act yields one observation (an HTTP status and
//! body, or an exit code and stdout) and the screen it reaches is checked against it. The preview
//! runner records the verdict exactly as it records a browser step, so `keel test validation`
//! and `keel test sabotage` read backend journeys unchanged.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::Value;

/// How long one act may take; a hung request or command is a failed act, never a wait for ever.
const ACT_LIMIT: Duration = Duration::from_secs(30);
/// The most of a response or stdout an observation keeps.
const MAX_BODY: u64 = 1024 * 1024;
const METHODS: &[&str] = &["GET", "HEAD", "OPTIONS", "POST", "PUT", "PATCH", "DELETE"];
const HTTP_EXPECTS: &[&str] = &["status", "json", "text"];
/// Request headers the runner owns; an act's `headers` may not set them (journey-flow 1.4.0).
const RESERVED_HEADERS: &[&str] = &["host", "content-length", "connection", "transfer-encoding"];
const CLI_EXPECTS: &[&str] = &["exit", "stdout"];

/// What one act observed.
#[derive(Debug, Default)]
pub(super) struct Observation {
    pub(super) status: Option<u16>,
    pub(super) exit: Option<i32>,
    pub(super) body: String,
    /// #1662: a command's stderr, kept with its stdout as the run's evidence.
    pub(super) stderr: String,
}

fn acts(flow: &Value) -> impl Iterator<Item = &Value> {
    flow["edges"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|edge| edge["acts"].as_array().into_iter().flatten())
}

fn backend_kind(act: &Value) -> bool {
    matches!(act["kind"].as_str(), Some("http" | "cli"))
}

/// A flow is a backend flow when any act is `http` or `cli`.
/// #1662: every `cli` act's argv in the flow, in edge order: what the owner confirms.
pub(super) fn cli_argv(flow: &Value) -> Vec<String> {
    acts(flow)
        .filter(|act| act["kind"] == "cli")
        .filter_map(|act| act["name"].as_str().map(str::to_owned))
        .collect()
}

pub(super) fn is_backend(flow: &Value) -> bool {
    acts(flow).any(backend_kind)
}

/// A backend flow with an `http` act needs the app at `base`; a `cli`-only flow does not.
pub(super) fn needs_app(flow: &Value) -> bool {
    acts(flow).any(|act| act["kind"] == "http")
}

/// Semantic findings for a backend flow: `(code, pointer, message)`.
pub(super) fn findings(flow: &Value) -> Vec<(&'static str, String, &'static str)> {
    let mut found = Vec::new();
    if !is_backend(flow) {
        return found;
    }
    for (i, edge) in flow["edges"].as_array().into_iter().flatten().enumerate() {
        for (a, act) in edge["acts"].as_array().into_iter().flatten().enumerate() {
            let pointer = format!("/edges/{i}/acts/{a}");
            let name = act["name"].as_str().unwrap_or_default();
            let valid = match act["kind"].as_str() {
                Some("http") => {
                    METHODS.contains(&act["role"].as_str().unwrap_or_default())
                        && name.starts_with('/')
                        && !name.starts_with("//")
                }
                Some("cli") => act["role"] == "exec" && !name.trim().is_empty(),
                _ => {
                    found.push((
                        "flow.observer_mixed",
                        pointer,
                        "a flow with http or cli acts cannot also play browser acts",
                    ));
                    continue;
                }
            };
            if act["headers"].as_object().is_some_and(|headers| {
                headers
                    .keys()
                    .any(|name| RESERVED_HEADERS.contains(&name.to_ascii_lowercase().as_str()))
            }) {
                found.push((
                    "flow.backend_header_reserved",
                    format!("{pointer}/headers"),
                    "an http act may not set Host, Content-Length, Connection or Transfer-Encoding",
                ));
            }
            if act["kind"] == "cli"
                && let Some((code, message)) = cli_refusal(name)
            {
                found.push((code, pointer.clone(), message));
            }
            if !valid {
                found.push((
                    "flow.backend_act_invalid",
                    pointer,
                    "http needs a method role and a /path name; cli needs role exec and argv",
                ));
            }
        }
    }
    for (i, screen) in flow["screens"].as_array().into_iter().flatten().enumerate() {
        for (e, expect) in screen["expect"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let role = expect["role"].as_str().unwrap_or_default();
            if !HTTP_EXPECTS.contains(&role) && !CLI_EXPECTS.contains(&role) {
                found.push((
                    "flow.backend_expect_invalid",
                    format!("/screens/{i}/expect/{e}/role"),
                    "a backend expectation is status, json, text, exit or stdout",
                ));
            }
        }
    }
    found
}

/// Sends one `http` act (or the start screen's GET) to `base` and reads the whole response.
pub(super) fn request(
    base: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
    headers: Option<&serde_json::Map<String, Value>>,
) -> Result<Observation, String> {
    let authority = base
        .split_once("://")
        .map_or(base, |(_, rest)| rest)
        .split('/')
        .next()
        .unwrap_or_default();
    let address = super::journey_live::base_address(base).ok_or("base has no address")?;
    let mut stream = std::net::ToSocketAddrs::to_socket_addrs(&address)
        .map_err(|error| error.to_string())?
        .find_map(|address| TcpStream::connect_timeout(&address, ACT_LIMIT).ok())
        .ok_or("connection refused")?;
    stream
        .set_read_timeout(Some(ACT_LIMIT))
        .and_then(|()| stream.set_write_timeout(Some(ACT_LIMIT)))
        .map_err(|error| error.to_string())?;
    let body = body.unwrap_or_default();
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\nAccept: */*\r\nContent-Length: {}\r\n",
        body.len()
    );
    let mut typed = false;
    for (name, value) in headers.into_iter().flatten() {
        let Some(value) = value.as_str() else {
            continue;
        };
        if value.contains(['\r', '\n'])
            || RESERVED_HEADERS.contains(&name.to_ascii_lowercase().as_str())
        {
            continue;
        }
        typed |= name.eq_ignore_ascii_case("content-type");
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if !body.is_empty() && !typed {
        head.push_str("Content-Type: application/json\r\n");
    }
    head.push_str("\r\n");
    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(body.as_bytes()))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    stream
        .take(MAX_BODY)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    let (headers, content) = text.split_once("\r\n\r\n").ok_or("malformed response")?;
    let status = headers
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or("malformed status line")?;
    let chunked = headers
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked");
    Ok(Observation {
        status: Some(status),
        exit: None,
        body: if chunked {
            dechunk(content)
        } else {
            content.to_owned()
        },
        stderr: String::new(),
    })
}

fn dechunk(mut rest: &str) -> String {
    let mut out = String::new();
    while let Some((size, tail)) = rest.split_once("\r\n") {
        let Ok(size) = usize::from_str_radix(size.split(';').next().unwrap_or("").trim(), 16)
        else {
            break;
        };
        if size == 0 || tail.len() < size {
            break;
        }
        out.push_str(&tail[..size]);
        rest = tail[size..].trim_start_matches("\r\n");
    }
    out
}

/// #2187: why an argv cannot be run as checked code, as `(code, message)`. Pure on the argv, so
/// lint and the runner refuse the same commands.
///
/// A program allowlist, not a deny list: the argv runs only a script file inside the project, the
/// thing the reviewed check reads. Allowed forms are `python[3][w]`/`py` with a script below the
/// project root (the root is the script's import directory and is not checked whole), `sh`,
/// `bash`, `dash` and `node` followed directly by a project script (no options: `-c`, `-x`,
/// `--require` and friends would name other code), `pwsh`/`powershell -File <script>`, or a
/// project-relative program path. Every other program (`git` with its `-c alias.x=!cmd`, `fish`,
/// `csh`, `Rscript`, `cscript`, `env`, an absolute path, ...) is refused, because none of them
/// is a file the check reads.
pub(super) fn cli_refusal(argv: &str) -> Option<(&'static str, &'static str)> {
    let mut words = argv.split_whitespace();
    let program = words.next()?;
    let args: Vec<&str> = words.collect();
    let stem = program_stem(program);
    let inline = (
        "flow.cli_inline_code",
        "a cli act must run a script file; -m, -c, -e, stdin and command-string forms name none",
    );
    let outside = (
        "flow.cli_script_outside",
        "a cli act's script must be a relative path inside the project",
    );
    if program.contains(['/', '\\']) {
        if script_directory_depth(program).is_none() {
            return Some(outside);
        }
        // A project-held program named like a runner or interpreter is one: run it by its
        // name instead, so the runner rules (options, script position, flags) apply.
        return is_interpreter(&stem).then_some((
            "flow.cli_program_interpreter",
            "a project-relative program may not be named like an interpreter; run the script through the runner by name",
        ));
    }
    if is_python(&stem) {
        let mut rest = args.iter();
        let script = loop {
            let Some(word) = rest.next() else {
                return Some(inline);
            };
            if *word == "-" {
                return Some(inline);
            }
            if word.starts_with("--") {
                continue;
            }
            if let Some(cluster) = word.strip_prefix('-') {
                for (i, flag) in cluster.char_indices() {
                    match flag {
                        'c' | 'm' => return Some(inline),
                        // The rest of the word, or the next word, is the option's value.
                        'W' | 'X' => {
                            if i + 1 == cluster.len() {
                                rest.next();
                            }
                            break;
                        }
                        _ => {}
                    }
                }
                continue;
            }
            break *word;
        };
        if script_directory_depth(script).is_none_or(|depth| depth == 0) {
            return Some((
                "flow.cli_root_script",
                "a python script at the project root imports unchecked root modules; move it into a directory",
            ));
        }
        return None;
    }
    let script = match stem.as_str() {
        "sh" | "bash" | "dash" | "node" | "nodejs" => args.first().copied(),
        "powershell" | "pwsh" => match args.as_slice() {
            [flag, script, ..] if matches!(flag.to_ascii_lowercase().as_str(), "-file" | "-f") => {
                Some(*script)
            }
            _ => return Some(inline),
        },
        _ => {
            return Some((
                "flow.cli_program_not_allowed",
                "a cli act runs a project script through python, sh, bash, dash, node or pwsh -File, or a project-relative program",
            ));
        }
    };
    match script {
        None => Some(inline),
        Some(word) if word.starts_with(['-', '+']) => Some(inline),
        Some(word) => script_directory_depth(word).is_none().then_some(outside),
    }
}

/// #2187: how many directories below the project root a script sits, read lexically (`.` dropped,
/// `..` pops); `None` when the path is absolute or climbs above the root.
fn script_directory_depth(script: &str) -> Option<usize> {
    if script.starts_with(['/', '\\']) || script.get(1..2) == Some(":") {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in script.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    parts.len().checked_sub(1)
}

fn program_stem(program: &str) -> String {
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let lower = name.to_ascii_lowercase();
    match lower.rsplit_once('.') {
        Some((stem, "exe" | "cmd" | "bat" | "com")) => stem.to_owned(),
        _ => lower,
    }
}

/// #2187: names of runners and interpreters that take code in their arguments. A
/// project-relative program with one of these stems is refused (lint and runner alike).
fn is_interpreter(stem: &str) -> bool {
    const NAMES: &[&str] = &[
        "sh",
        "bash",
        "dash",
        "zsh",
        "ksh",
        "mksh",
        "fish",
        "csh",
        "tcsh",
        "ash",
        "busybox",
        "node",
        "nodejs",
        "deno",
        "bun",
        "perl",
        "ruby",
        "irb",
        "php",
        "lua",
        "luajit",
        "tclsh",
        "wish",
        "pwsh",
        "powershell",
        "cmd",
        "rscript",
        "r",
        "awk",
        "gawk",
        "mawk",
        "nawk",
        "sed",
        "env",
        "xargs",
        "git",
        "osascript",
        "cscript",
        "wscript",
        "mshta",
        "java",
        "jshell",
        "dotnet",
        "julia",
        "guile",
        "racket",
        "expect",
        "make",
        "find",
    ];
    is_python(stem) || NAMES.contains(&stem)
}

fn is_python(stem: &str) -> bool {
    let Some(rest) = stem
        .strip_prefix("pythonw")
        .or_else(|| stem.strip_prefix("python"))
    else {
        return stem == "py" || stem == "pyw";
    };
    rest.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// #2187: the variables a step keeps from the Runtime's environment; every other one (PYTHONPATH,
/// PYTHONSTARTUP, NODE_OPTIONS, BASH_ENV, LD_PRELOAD, ...) is dropped. None of these names code.
const KEPT_ENV: &[&str] = &[
    "SYSTEMROOT",
    "WINDIR",
    "SYSTEMDRIVE",
    "COMSPEC",
    "PATHEXT",
    "TEMP",
    "TMP",
    "TMPDIR",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "HOME",
    "LOCALAPPDATA",
    "APPDATA",
    "PROGRAMDATA",
    "PROGRAMFILES",
    "PROGRAMFILES(X86)",
    "LANG",
    "LC_ALL",
    "USER",
    "USERNAME",
];

/// #2187: the PATH a step resolves its program against and inherits: the Runtime's PATH without
/// relative entries and without any entry inside the project (a program planted there runs
/// unchecked).
fn pinned_path(project: &Path, path: Option<std::ffi::OsString>) -> Vec<PathBuf> {
    let root = std::fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    path.map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter(|dir| dir.is_absolute())
        .filter(|dir| {
            let real = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.clone());
            !real.starts_with(&root)
        })
        .collect()
}

/// #2187: a bare program name resolved to one absolute file in `dirs`; a name with a path
/// separator is a project-relative program (what the reviewed check reads), joined to `project`.
fn resolve_program(project: &Path, program: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    if program.contains(['/', '\\']) {
        return Some(project.join(program));
    }
    let names: Vec<String> = if cfg!(windows) && !program.contains('.') {
        ["exe", "com", "cmd", "bat"]
            .iter()
            .map(|ext| format!("{program}.{ext}"))
            .collect()
    } else {
        vec![program.to_owned()]
    };
    dirs.iter()
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

/// #2187: whether `program` (already joined to the project) is a real file inside the project
/// once links and junctions are resolved.
fn resolves_inside(project: &Path, program: &Path) -> bool {
    let root = std::fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    std::fs::canonicalize(program).is_ok_and(|real| real.starts_with(&root))
}

/// #2187: the first bytes of a native executable (PE, ELF, Mach-O and its fat form). A
/// project-relative program that starts with one is a binary the review cannot read, such as a
/// byte copy or hard link of an interpreter under a neutral name.
fn is_native_binary(program: &Path) -> bool {
    let mut head = [0u8; 4];
    let read = std::fs::File::open(program)
        .and_then(|mut file| file.read(&mut head))
        .unwrap_or(0);
    let head = &head[..read];
    head.starts_with(b"MZ")
        || head == b"\x7fELF"
        || matches!(
            head,
            [0xfe, 0xed, 0xfa, 0xce | 0xcf]
                | [0xce | 0xcf, 0xfa, 0xed, 0xfe]
                | [0xca, 0xfe, 0xba, 0xbe]
                | [0xbe, 0xba, 0xfe, 0xca]
        )
}

/// #2187: the most shell files one step's `.`/`source` lines are followed through.
const SOURCED_FILES_LIMIT: usize = 64;

/// #2187: the first `.`/`source` target in a shell script (followed through the files it names)
/// that the reviewed check does not cover, or `None`. The check vouches for the script's own
/// directory, and the shell resolves a relative target against the working directory (the project
/// root) or PATH, so a target is accepted only as a literal relative path with a `/`, no `..`,
/// no expansion characters, inside the script's directory. An unreadable file, a target the
/// lexer cannot read as a literal, or more than [`SOURCED_FILES_LIMIT`] files answer `Some`.
fn unchecked_sourced_file(project: &Path, script: &str) -> Option<String> {
    let parts = |path: &str| -> Option<Vec<String>> {
        let mut out = Vec::new();
        for part in path.split(['/', '\\']) {
            match part {
                "" | "." => {}
                ".." => return None,
                part => out.push(part.to_owned()),
            }
        }
        Some(out)
    };
    let script_parts = parts(script)?;
    let dir = &script_parts[..script_parts.len().saturating_sub(1)];
    let mut queue = vec![script_parts.join("/")];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(file) = queue.pop() {
        if !seen.insert(file.clone()) {
            continue;
        }
        if seen.len() > SOURCED_FILES_LIMIT {
            return Some(file);
        }
        let Ok(text) = std::fs::read_to_string(project.join(&file)) else {
            return Some(file);
        };
        for target in sourced_targets(&text) {
            let inside = !target.starts_with(['/', '\\'])
                && target.contains('/')
                && !target.contains(['$', '`', '"', '\'', '*', '?', '[', '~', '{', '\\'])
                && !dir.is_empty()
                && parts(&target).is_some_and(|p| p.len() > dir.len() && p.starts_with(dir));
            if !inside {
                return Some(target);
            }
            queue.push(parts(&target).unwrap_or_default().join("/"));
        }
    }
    None
}

/// #2187: the word after each `.`/`source` command in shell text (an empty word when the line
/// ends there). Lexical: comments are dropped, commands split on `;`, `&`, `|`, parentheses and
/// braces, and the `builtin`/`command` prefixes and leading keywords are skipped.
fn sourced_targets(text: &str) -> Vec<String> {
    let mut targets = Vec::new();
    for line in text.lines() {
        let line = match line.find(" #") {
            Some(at) => &line[..at],
            None if line.trim_start().starts_with('#') => "",
            None => line,
        };
        for segment in line.split([';', '&', '|', '(', ')', '{', '}']) {
            let mut words = segment.split_whitespace().skip_while(|word| {
                matches!(
                    *word,
                    "builtin" | "command" | "then" | "do" | "else" | "!" | "if" | "while" | "until"
                )
            });
            if let Some(".") | Some("source") = words.next() {
                targets.push(words.next().unwrap_or_default().to_owned());
            }
        }
    }
    targets
}

/// Runs one `cli` act: argv split on whitespace, no shell, in the project, bounded by
/// [`ACT_LIMIT`]. The child is killed at the limit.
///
/// #2187: fail closed on what the reviewed check cannot see. The argv must pass [`cli_refusal`];
/// the environment is cleared to [`KEPT_ENV`] plus a pinned PATH; a bare program resolves only
/// through that PATH; Python runs with `-E -s -S -B` (no `PYTHON*` variables, no user or system
/// site-packages and so no `.pth` file, no bytecode written).
pub(super) fn command(project: &Path, argv: &str) -> Result<Observation, String> {
    if let Some((code, message)) = cli_refusal(argv) {
        return Err(format!("{code}: {message}"));
    }
    let mut words = argv.split_whitespace();
    let program = words.next().ok_or("empty argv")?;
    let dirs = pinned_path(project, std::env::var_os("PATH"));
    let resolved = resolve_program(project, program, &dirs).ok_or_else(|| {
        "flow.cli_program_unresolved: the program is not on the pinned PATH \
         (absolute entries outside the project)"
            .to_owned()
    })?;
    if program.contains(['/', '\\']) && !resolves_inside(project, &resolved) {
        // A link (or a junction on its way) that leaves the project names a program the check
        // never read: the same code lint gives a path outside the project.
        return Err(
            "flow.cli_script_outside: a cli act's program resolves outside the project".into(),
        );
    }
    if program.contains(['/', '\\']) && is_native_binary(&resolved) {
        return Err(
            "flow.cli_program_binary: a project-relative program is a native executable \
             the review cannot read; run a project script through a runner"
                .into(),
        );
    }
    let stem = program_stem(program);
    if matches!(stem.as_str(), "sh" | "bash" | "dash") {
        let script = words.clone().next().unwrap_or_default();
        if let Some(target) = unchecked_sourced_file(project, script) {
            return Err(format!(
                "flow.cli_script_sources: a shell script sources `{target}`, which is not a \
                 literal path inside the script's directory"
            ));
        }
    }
    let mut spawn = graphhelm_process::command(&resolved);
    spawn.env_clear();
    for (key, value) in std::env::vars_os() {
        let upper = key.to_string_lossy().to_ascii_uppercase();
        if KEPT_ENV.contains(&upper.as_str()) {
            spawn.env(key, value);
        }
    }
    if let Ok(path) = std::env::join_paths(&dirs) {
        spawn.env("PATH", path);
    }
    let words: Vec<&str> = words.collect();
    if is_python(&stem) {
        spawn.args(["-E", "-s", "-S", "-B"]);
    }
    if matches!(stem.as_str(), "pwsh" | "powershell") {
        // No profile script runs, and nothing waits on a prompt.
        spawn.args(["-NoProfile", "-NonInteractive"]);
    }
    if matches!(stem.as_str(), "node" | "nodejs") {
        // Node's permission model: the script reads only its own directory, so a module
        // required from outside it (a parent `node_modules`, a sibling folder) fails to load;
        // no child process, worker, addon or write is granted.
        let script = words.first().copied().unwrap_or_default();
        let dir = Path::new(script).parent().unwrap_or(Path::new(""));
        let dir = std::fs::canonicalize(project.join(dir)).unwrap_or_else(|_| project.join(dir));
        let dir = dir.join("*").display().to_string();
        // Node does not read the verbatim `\\?\` prefix Windows canonical paths carry.
        let dir = dir.strip_prefix(r"\\?\").unwrap_or(&dir);
        spawn.arg("--permission");
        spawn.arg(format!("--allow-fs-read={dir}"));
    }
    let mut child = spawn
        .args(&words)
        // #2008: a run must not write `__pycache__` into the reviewed tree it was vouched by.
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .current_dir(project)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let mut stderr = child.stderr.take().ok_or("no stderr")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = (&mut stdout).take(MAX_BODY).read_to_end(&mut bytes);
        bytes
    });
    // #1662: stderr is evidence; a failed read says so in place of the bytes.
    let errors = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        match (&mut stderr).take(MAX_BODY).read_to_end(&mut bytes) {
            Ok(_) => bytes,
            Err(error) => format!("stderr unreadable: {error}").into_bytes(),
        }
    });
    let deadline = Instant::now() + ACT_LIMIT;
    let status = loop {
        match child.try_wait().map_err(|error| error.to_string())? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("command timed out".into());
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let bytes = reader.join().unwrap_or_default();
    let error_bytes = errors
        .join()
        .unwrap_or_else(|_| b"stderr unreadable: reader panicked".to_vec());
    Ok(Observation {
        status: None,
        exit: status.code(),
        body: String::from_utf8_lossy(&bytes).into_owned(),
        stderr: String::from_utf8_lossy(&error_bytes).into_owned(),
    })
}

/// Plays one backend act.
pub(super) fn play(project: &Path, base: &str, act: &Value) -> Result<Observation, String> {
    let name = act["name"].as_str().unwrap_or_default();
    match act["kind"].as_str() {
        Some("http") => request(
            base,
            act["role"].as_str().unwrap_or("GET"),
            name,
            act["text"].as_str(),
            act["headers"].as_object(),
        ),
        Some("cli") => command(project, name),
        _ => Err("not a backend act".into()),
    }
}

fn json_field<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(value, |node, key| match node {
        Value::Array(items) => items.get(key.parse::<usize>().ok()?),
        _ => node.get(key),
    })
}

fn normal(text: &str) -> String {
    text.split(|c: char| c.is_whitespace() || c == '\u{a0}')
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The first expectation of `screen` the observation does not meet, as `role name`.
pub(super) fn unmet(screen: &Value, seen: &Observation) -> Option<String> {
    screen["expect"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|expect| {
            let name = expect["name"].as_str().unwrap_or_default();
            let met = match expect["role"].as_str().unwrap_or_default() {
                "status" => seen.status.map(|s| s.to_string()).as_deref() == Some(name),
                "exit" => seen.exit.map(|s| s.to_string()).as_deref() == Some(name),
                "text" | "stdout" => seen.body.contains(name),
                "json" => {
                    let (path, want) = name
                        .split_once('=')
                        .map_or((name, None), |(p, v)| (p, Some(v)));
                    serde_json::from_str::<Value>(&seen.body)
                        .ok()
                        .and_then(|body| json_field(&body, path).cloned())
                        .is_some_and(|field| {
                            want.is_none_or(|want| match &field {
                                Value::String(text) => text == want,
                                other => {
                                    serde_json::from_str::<Value>(want).ok().as_ref() == Some(other)
                                }
                            })
                        })
                }
                _ => false,
            };
            // #1041: a backend observation is a whole, finished response, so absence is judged
            // on it directly: an `absent` expectation is unmet exactly when it is present.
            // #1781: a `text` expectation also needs the finished body to contain that text,
            // whitespace collapsed and NBSP read as a space, as the browser driver compares.
            let met = met
                && expect["text"]
                    .as_str()
                    .is_none_or(|text| normal(&seen.body).contains(&normal(text)));
            if expect["absent"] == true { met } else { !met }
        })
        .map(|expect| {
            format!(
                "{}{} {}",
                if expect["absent"] == true { "no " } else { "" },
                expect["role"].as_str().unwrap_or_default(),
                expect["name"].as_str().unwrap_or_default()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The defect named (ml-saas trial): an http act's `headers` (a tenant id) never reached the
    /// app, and a reserved header could smuggle a second Host or a forged length. Cost: one
    /// loopback socket, no network.
    /// #1041: a backend observation is a finished response, so `absent` inverts the match there:
    /// a body that still carries the text fails it, and one without passes. Cost: in memory.
    #[test]
    fn an_absent_backend_expectation_fails_while_present() {
        let screen = json!({"expect":[{"role":"status","name":"200"},
            {"role":"text","name":"error","absent":true}]});
        let seen = |body: &str| Observation {
            status: Some(200),
            exit: None,
            body: body.to_owned(),
            stderr: String::new(),
        };
        assert_eq!(unmet(&screen, &seen("all good")), None);
        assert_eq!(
            unmet(&screen, &seen("an error")),
            Some("no text error".to_owned())
        );
    }

    /// #1781: a backend `text` expectation is met only when the finished body contains the
    /// text, whitespace collapsed and NBSP read as a space. Cost: in memory.
    #[test]
    fn a_backend_text_expectation_needs_the_text_in_the_body() {
        let screen = json!({"expect":[{"role":"status","name":"200","text":"R$ 246,90"}]});
        let seen = |body: &str| Observation {
            status: Some(200),
            exit: None,
            body: body.to_owned(),
            stderr: String::new(),
        };
        assert_eq!(unmet(&screen, &seen("total:\n R$\u{a0}246,90")), None);
        assert_eq!(
            unmet(&screen, &seen("total: R$ 24,69")),
            Some("status 200".to_owned())
        );
    }

    #[test]
    fn http_act_sends_its_headers_but_never_reserved_ones() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut seen = Vec::new();
            let mut buf = [0u8; 1024];
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = socket.read(&mut buf).unwrap();
                seen.extend_from_slice(&buf[..n]);
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .unwrap();
            String::from_utf8(seen).unwrap()
        });
        let act = json!({"kind":"http","role":"GET","name":"/x",
            "headers":{"x-tenant-id":"t1","Host":"evil"}});
        let seen = play(Path::new("."), &base, &act).unwrap();
        assert_eq!(seen.status, Some(200));
        let request = server.join().unwrap();
        assert!(request.contains("x-tenant-id: t1\r\n"), "{request}");
        assert!(!request.contains("evil"), "{request}");
        assert_eq!(
            findings(&json!({"edges":[{"acts":[act]}]}))[0].0,
            "flow.backend_header_reserved"
        );
    }

    /// The defect named: a backend expectation that passes on the wrong value (a status prefix,
    /// a JSON field compared as its quoted form, a missing field read as present).
    #[test]
    fn expectations_match_exact_values_and_report_the_first_unmet() {
        let screen = json!({"expect":[{"role":"status","name":"200"},
            {"role":"json","name":"item.tags.1=b"},{"role":"json","name":"count=2"},
            {"role":"text","name":"apple"}]});
        let seen = Observation {
            status: Some(200),
            exit: None,
            body: r#"{"item":{"name":"apple","tags":["a","b"]},"count":2}"#.into(),
            stderr: String::new(),
        };
        assert_eq!(unmet(&screen, &seen), None);
        let wrong = Observation {
            status: Some(2000),
            ..Default::default()
        };
        assert_eq!(unmet(&screen, &wrong).as_deref(), Some("status 200"));
        let missing = Observation {
            status: Some(200),
            exit: None,
            body: r#"{"item":{"tags":["a","c"]},"count":2}"#.into(),
            stderr: String::new(),
        };
        assert_eq!(
            unmet(&screen, &missing).as_deref(),
            Some("json item.tags.1=b")
        );
        assert_eq!(dechunk("3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n"), "abcde");
    }

    /// #2187 gaps 2 and 3: a command whose code is no file the reviewed check reads, or a Python
    /// script whose import directory is the unchecked project root, is refused with a stable code
    /// by lint and by the runner alike. Controls: a script in a directory, with valued options
    /// before it, and a plain `sh` script are not refused.
    #[test]
    fn a_cli_act_without_a_checked_script_is_refused() {
        let inline = Some("flow.cli_inline_code");
        for (argv, code) in [
            ("python -m http.server", inline),
            ("python3 -c print(1)", inline),
            ("python -Bc print(1)", inline),
            ("py -3 -m pip", inline),
            ("python -", inline),
            ("python", inline),
            ("sh -c id", inline),
            ("bash -ec id", inline),
            ("node -e 1", inline),
            ("cmd /c dir", Some("flow.cli_program_not_allowed")),
            ("pwsh -Command id", inline),
            ("sh -o pipefail -c id", inline),
            ("bash --init-file x -c id", inline),
            (
                "env python -c print(1)",
                Some("flow.cli_program_not_allowed"),
            ),
            (
                "env PYTHONPATH=x python d/run.py",
                Some("flow.cli_program_not_allowed"),
            ),
            ("xargs python", Some("flow.cli_program_not_allowed")),
            ("timeout 5 sh -c id", Some("flow.cli_program_not_allowed")),
            ("perl -e 1", Some("flow.cli_program_not_allowed")),
            ("python run.py", Some("flow.cli_root_script")),
            ("python ./run.py", Some("flow.cli_root_script")),
            ("python sub/../json.py", Some("flow.cli_root_script")),
            ("python ../outside.py", Some("flow.cli_root_script")),
            ("python ./d/./run.py", None),
            ("python.exe -W ignore json.py", Some("flow.cli_root_script")),
            ("python .graphhelm/fixtures/x/run.py a", None),
            ("python -W ignore -X utf8 tools/x.py", None),
            ("sh greet.sh", None),
            ("pwsh -File tools/x.ps1", None),
            ("git status", Some("flow.cli_program_not_allowed")),
            ("git -c alias.x=!id x", Some("flow.cli_program_not_allowed")),
            ("fish -c id", Some("flow.cli_program_not_allowed")),
            ("csh -c id", Some("flow.cli_program_not_allowed")),
            ("tcsh -c id", Some("flow.cli_program_not_allowed")),
            ("mksh -c id", Some("flow.cli_program_not_allowed")),
            ("Rscript -e 1", Some("flow.cli_program_not_allowed")),
            ("cscript x.vbs", Some("flow.cli_program_not_allowed")),
            ("sh -x d/s.sh", inline),
            ("sh", inline),
            ("node --require ./x.js d/a.js", inline),
            ("node d/a.js", None),
            ("sh ../s.sh", Some("flow.cli_script_outside")),
            ("pwsh -File ../x.ps1", Some("flow.cli_script_outside")),
            ("/usr/bin/perl -e 1", Some("flow.cli_script_outside")),
            ("C:/x/perl.exe", Some("flow.cli_script_outside")),
            ("./tools/bin/probe a", None),
            ("./bin/sh -c id", Some("flow.cli_program_interpreter")),
            ("tools/node.exe -e 1", Some("flow.cli_program_interpreter")),
            (
                "./bin/python3.12 x.py",
                Some("flow.cli_program_interpreter"),
            ),
            ("./bin/awk BEGIN{}", Some("flow.cli_program_interpreter")),
            ("nodejs d/a.js", None),
        ] {
            assert_eq!(cli_refusal(argv).map(|(code, _)| code), code, "{argv}");
            let flow = json!({"edges":[{"acts":[{"kind":"cli","role":"exec","name":argv}]}]});
            let found: Vec<_> = findings(&flow).into_iter().map(|f| (f.0, f.1)).collect();
            let expected: Vec<_> = code
                .map(|code| (code, "/edges/0/acts/0".to_owned()))
                .into_iter()
                .collect();
            assert_eq!(found, expected, "{argv}");
            if let Some(code) = code {
                let refused = command(Path::new("."), argv).unwrap_err();
                assert!(refused.starts_with(code), "{argv}: {refused}");
            }
        }
    }

    /// #2187 gap 4: a program is found only through absolute PATH entries outside the project;
    /// one found nowhere there is refused with a stable code instead of being looked up anyway.
    #[test]
    fn a_cli_program_resolves_only_through_the_pinned_path() {
        let project = tempfile::tempdir().unwrap();
        let inside = project.path().join("bin");
        std::fs::create_dir_all(&inside).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let path = std::env::join_paths([
            PathBuf::from("relative-bin"),
            inside.clone(),
            outside.path().to_path_buf(),
        ])
        .unwrap();
        let dirs = pinned_path(project.path(), Some(path));
        assert_eq!(dirs, vec![outside.path().to_path_buf()]);
        let refused = command(project.path(), "python3.2187 d/x.py").unwrap_err();
        assert!(
            refused.starts_with("flow.cli_program_unresolved"),
            "{refused}"
        );
    }

    /// #2187 gap 1: the step does not inherit the Runtime's environment. The test process carries
    /// variables outside the kept list (Cargo sets `CARGO_*` for every test); none reaches the
    /// child, a project Python script that prints the environment it was started with.
    #[test]
    fn a_cli_step_sees_only_the_kept_environment() {
        assert!(std::env::vars().any(|(key, _)| key.starts_with("CARGO")));
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join("d")).unwrap();
        std::fs::write(
            project.path().join("d/env.py"),
            "import os
for key in os.environ:
    print('ENV:' + key + '=')
",
        )
        .unwrap();
        let seen = command(project.path(), "python d/env.py").unwrap();
        assert_eq!(seen.exit, Some(0), "{}", seen.stderr);
        let keys: Vec<String> = seen
            .body
            .lines()
            .filter_map(|line| line.strip_prefix("ENV:"))
            .map(|line| line.split_once('=').map_or(line, |(key, _)| key).to_owned())
            .collect();
        assert!(
            !keys.is_empty(),
            "the child printed no environment: {}",
            seen.body
        );
        for key in &keys {
            let upper = key.to_ascii_uppercase();
            assert!(
                !upper.starts_with("CARGO") && !upper.starts_with("RUST"),
                "inherited {key}"
            );
        }
        assert!(keys.iter().any(|key| key.eq_ignore_ascii_case("PATH")));
    }

    /// #2187 gap 5: a Python step runs with `-E -s -S -B` (no site-packages, so no `.pth`), and a module planted at the project root
    /// (outside the script's checked directory) is not importable from the script.
    #[test]
    fn a_python_step_cannot_import_from_outside_its_directory() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join("d")).unwrap();
        std::fs::write(project.path().join("planted2187.py"), "print('PLANTED')\n").unwrap();
        std::fs::write(
            project.path().join("d/run.py"),
            "import sys\n\
             print(*(int(f) for f in (sys.flags.ignore_environment, sys.flags.no_user_site, sys.flags.no_site, sys.flags.dont_write_bytecode)))\n\
             try:\n    import planted2187\nexcept ImportError:\n    print('blocked')\n",
        )
        .unwrap();
        let seen = command(project.path(), "python d/run.py").unwrap();
        assert_eq!(seen.exit, Some(0), "{}", seen.stderr);
        assert_eq!(
            seen.body.split_whitespace().collect::<Vec<_>>(),
            ["1", "1", "1", "1", "blocked"]
        );
    }

    /// #2187: a project-relative program that is a link leaving the project runs code the check
    /// never read; the runner refuses it with the code lint gives an outside path.
    #[cfg(unix)]
    #[test]
    fn a_project_program_linked_outside_is_refused() {
        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("tool");
        std::fs::write(
            &target,
            "#!/bin/sh
echo OUT
",
        )
        .unwrap();
        std::fs::create_dir_all(project.path().join("bin")).unwrap();
        std::os::unix::fs::symlink(&target, project.path().join("bin/tool")).unwrap();
        let refused = command(project.path(), "./bin/tool").unwrap_err();
        assert!(refused.starts_with("flow.cli_script_outside"), "{refused}");
    }

    /// #2187: PowerShell runs a project script with `-NoProfile -NonInteractive`, so no profile
    /// script loads. Windows ships Windows PowerShell, so the proof needs nothing installed.
    #[cfg(windows)]
    #[test]
    fn a_powershell_step_loads_no_profile() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join("d")).unwrap();
        std::fs::write(
            project.path().join("d/args.ps1"),
            "[Environment]::GetCommandLineArgs() -join ' '
",
        )
        .unwrap();
        let seen = command(project.path(), "powershell -File d/args.ps1").unwrap();
        assert_eq!(seen.exit, Some(0), "{}", seen.stderr);
        let args = seen.body.to_ascii_lowercase();
        assert!(
            args.contains("-noprofile") && args.contains("-noninteractive"),
            "{args}"
        );
    }

    /// #2187: a node step reads only its script's directory: a module beside the script loads,
    /// one in a parent folder does not. Needs `node` (with the permission model) on PATH; the
    /// test says so and stops when the machine has none.
    #[test]
    fn a_node_step_cannot_require_from_outside_its_directory() {
        let project = tempfile::tempdir().unwrap();
        let dirs = pinned_path(project.path(), std::env::var_os("PATH"));
        if resolve_program(project.path(), "node", &dirs).is_none() {
            eprintln!("OBSERVER_MISSING: node is not on PATH");
            return;
        }
        std::fs::create_dir_all(project.path().join("d")).unwrap();
        std::fs::write(
            project.path().join("planted2187.js"),
            "module.exports='PLANTED';
",
        )
        .unwrap();
        std::fs::write(
            project.path().join("d/near.js"),
            "module.exports='near';
",
        )
        .unwrap();
        std::fs::write(
            project.path().join("d/run.js"),
            "console.log(require('./near.js'));
             try { console.log(require('../planted2187.js')); } catch (e) { console.log('blocked'); }
",
        )
        .unwrap();
        let seen = command(project.path(), "node d/run.js").unwrap();
        assert_eq!(seen.exit, Some(0), "{}", seen.stderr);
        assert_eq!(
            seen.body.split_whitespace().collect::<Vec<_>>(),
            ["near", "blocked"]
        );
    }

    /// #2187: a project-relative program that is a native executable (a byte copy or hard link
    /// of an interpreter under a neutral name) is refused before it runs, on every platform.
    #[test]
    fn a_project_program_that_is_a_native_binary_is_refused() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join("bin")).unwrap();
        for (name, head) in [
            ("tool.exe", &b"MZ\x90\x00"[..]),
            ("tool", &b"\x7fELF\x02\x01"[..]),
            ("mach", &b"\xcf\xfa\xed\xfe"[..]),
        ] {
            std::fs::write(project.path().join("bin").join(name), head).unwrap();
            let refused = command(project.path(), &format!("./bin/{name}")).unwrap_err();
            assert!(
                refused.starts_with("flow.cli_program_binary"),
                "{name}: {refused}"
            );
        }
    }

    /// #2187: a shell script's `.`/`source` targets are followed; only a literal path inside the
    /// script's own (reviewed) directory passes, and the files it sources are followed in turn.
    #[test]
    fn a_shell_script_may_source_only_files_in_its_directory() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join("d/lib")).unwrap();
        std::fs::write(project.path().join("d/lib/ok.sh"), "echo ok\n").unwrap();
        std::fs::write(project.path().join("d/lib/deep.sh"), ". ./planted.sh\n").unwrap();
        let cases = [
            ("# . ../x.sh\n. ./d/lib/ok.sh\necho hi\n", None),
            ("source d/lib/ok.sh; echo done\n", None),
            (". ./planted.sh\n", Some("./planted.sh")),
            ("if true; then source ../x.sh; fi\n", Some("../x.sh")),
            ("builtin . /etc/profile\n", Some("/etc/profile")),
            (". lib.sh\n", Some("lib.sh")),
            (". \"$(dirname \"$0\")/lib/ok.sh\"\n", Some("\"$")),
            ("true && . ./d/lib/deep.sh\n", Some("./planted.sh")),
            (".\n", Some("")),
        ];
        for (text, expected) in cases {
            std::fs::write(project.path().join("d/run.sh"), text).unwrap();
            assert_eq!(
                unchecked_sourced_file(project.path(), "d/run.sh").as_deref(),
                expected,
                "{text}"
            );
        }
        // A project-root script is reviewed alone, so it may source nothing.
        std::fs::write(project.path().join("root.sh"), ". ./d/lib/ok.sh\n").unwrap();
        assert_eq!(
            unchecked_sourced_file(project.path(), "root.sh").as_deref(),
            Some("./d/lib/ok.sh")
        );
    }

    /// #2187: the runner refuses a shell step whose script sources an unchecked file. Needs `sh`
    /// on the pinned PATH; the test says so and stops when the machine has none.
    #[test]
    fn a_shell_step_that_sources_outside_its_directory_is_refused() {
        let project = tempfile::tempdir().unwrap();
        let dirs = pinned_path(project.path(), std::env::var_os("PATH"));
        if resolve_program(project.path(), "sh", &dirs).is_none() {
            eprintln!("OBSERVER_MISSING: sh is not on PATH");
            return;
        }
        std::fs::create_dir_all(project.path().join("d")).unwrap();
        std::fs::write(project.path().join("planted.sh"), "echo PLANTED\n").unwrap();
        std::fs::write(project.path().join("d/run.sh"), ". ./planted.sh\n").unwrap();
        let refused = command(project.path(), "sh d/run.sh").unwrap_err();
        assert!(refused.starts_with("flow.cli_script_sources"), "{refused}");
    }
}
