//! Keel's line rules: reliability and naming breaks judged on the lines a diff adds.
//!
//! Only added production lines are judged, so code that already exists never blocks a change (the
//! ratchet). Test code is out of scope: `#[cfg(test)]` items in Rust and every test path
//! (`tests/`, `*.test.ts(x)`, `test_*.py`, ...). A finding is excused only by a comment on its line
//! or the line above, `keel-allow: <rule id> <reason> until <YYYY-MM-DD|#issue>`, and every excuse
//! that was honoured is reported beside the findings. The rule ids are the ones `KEEL.md` lists.
//!
//! The patterns are lexical and deliberately narrow: each one names a shape that is wrong almost
//! wherever it appears, so a finding is worth reading rather than tuning out. Their hit rate on the
//! last fifty merged pull requests is recorded in the pull request that added them.

use serde::{Deserialize, Serialize};

use crate::keel::{Finding, is_test_path};

pub const NO_PANIC: &str = "keel.rel.no_panic";
pub const NO_SWALLOW: &str = "keel.rel.no_swallow";
pub const DEADLINE: &str = "keel.rel.deadline";
pub const NAMES: &str = "keel.names";
pub const NO_BARE_FLAG: &str = "keel.names.no_bare_flag";

/// An excuse the check honoured: where it sits, what it excuses, why and until when.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HonouredException {
    pub rule: String,
    pub path: String,
    pub line: u32,
    pub reason: String,
    pub until: String,
}

/// The line rules' verdict on one diff.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineReport {
    pub findings: Vec<Finding>,
    pub exceptions: Vec<HonouredException>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Language {
    Rust,
    TypeScript,
    Python,
}

impl Language {
    fn of(path: &str) -> Option<Self> {
        match path.rsplit('.').next()? {
            "rs" => Some(Self::Rust),
            "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" => Some(Self::TypeScript),
            "py" | "pyi" => Some(Self::Python),
            _ => None,
        }
    }
}

/// Judges every added production line of `diff`. `head_text` returns a changed file's text at the
/// head commit: it places a line inside or outside a `#[cfg(test)]` item, reads the line above for
/// an excuse and gives the rules a few lines of context. A file it cannot return is judged on its
/// added lines alone. `today` is `YYYY-MM-DD`; an excuse dated before it has expired.
#[must_use]
pub fn check_lines(
    diff: &str,
    head_text: &dyn Fn(&str) -> Option<String>,
    today: &str,
) -> LineReport {
    let mut report = LineReport::default();
    for file in added_lines(diff) {
        let Some(language) = Language::of(&file.path) else {
            continue;
        };
        if is_test_path(&file.path) {
            continue;
        }
        let text = head_text(&file.path);
        let lines: Vec<&str> = text
            .as_deref()
            .map_or_else(Vec::new, |text| text.lines().collect());
        let test_lines = if language == Language::Rust {
            cfg_test_lines(&lines)
        } else {
            Vec::new()
        };
        for (number, raw) in &file.added {
            let index = usize::try_from(*number)
                .unwrap_or(usize::MAX)
                .saturating_sub(1);
            if test_lines.get(index).copied().unwrap_or(false) {
                continue;
            }
            let code = strip(language, raw);
            if code.trim().is_empty() {
                continue;
            }
            let context = window(&lines, index);
            let above = index
                .checked_sub(1)
                .and_then(|above| lines.get(above))
                .copied();
            for (rule, fix) in rules(language, &code, raw, &context, &lines, index) {
                match excuse(raw, above, rule, today) {
                    Excuse::Honoured { reason, until } => {
                        report.exceptions.push(HonouredException {
                            rule: rule.into(),
                            path: file.path.clone(),
                            line: *number,
                            reason,
                            until,
                        })
                    }
                    Excuse::Expired(until) => report.findings.push(finding(
                        rule,
                        &file.path,
                        *number,
                        raw,
                        &format!("the keel-allow for this line expired on {until}; {fix}"),
                    )),
                    Excuse::None => report
                        .findings
                        .push(finding(rule, &file.path, *number, raw, fix)),
                }
            }
        }
    }
    report
}

fn finding(rule: &str, path: &str, line: u32, raw: &str, fix: &str) -> Finding {
    let snippet: String = raw.trim().chars().take(120).collect();
    Finding {
        rule: rule.into(),
        path: Some(path.into()),
        detail: format!("{path}:{line}: `{snippet}`; fix: {fix}"),
        blocking: true,
    }
}

struct AddedFile {
    path: String,
    /// (line number at the head, text without the leading `+`).
    added: Vec<(u32, String)>,
}

/// The added lines of a unified diff with their line numbers on the new side.
fn added_lines(diff: &str) -> Vec<AddedFile> {
    let mut files: Vec<AddedFile> = Vec::new();
    let mut next: u32 = 0;
    let mut in_hunk = false;
    for raw in diff.lines() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.starts_with("diff --git ") {
            in_hunk = false;
            continue;
        }
        if let Some(path) = line.strip_prefix("+++ ") {
            in_hunk = false;
            let path = path.split('\t').next().unwrap_or(path);
            if path == "/dev/null" {
                continue;
            }
            files.push(AddedFile {
                path: path.strip_prefix("b/").unwrap_or(path).to_owned(),
                added: Vec::new(),
            });
            continue;
        }
        if let Some(header) = line.strip_prefix("@@ ") {
            // `@@ -a,b +c,d @@`: the new side starts at line c.
            next = header
                .split_whitespace()
                .find_map(|part| part.strip_prefix('+'))
                .and_then(|range| range.split(',').next())
                .and_then(|start| start.parse().ok())
                .unwrap_or(0);
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if let Some(added) = line.strip_prefix('+') {
            file.added.push((next, added.to_owned()));
            next = next.saturating_add(1);
        } else if line.starts_with(' ') || line.is_empty() {
            next = next.saturating_add(1);
        }
    }
    files
}

/// Which lines of a Rust file sit inside a `#[cfg(test)]` item, found by brace matching.
fn cfg_test_lines(lines: &[&str]) -> Vec<bool> {
    let mut inside = vec![false; lines.len()];
    let mut index = 0;
    while index < lines.len() {
        if !lines[index].trim_start().starts_with("#[cfg(test)]") {
            index += 1;
            continue;
        }
        let mut depth: i64 = 0;
        let mut opened = false;
        let mut end = index;
        while end < lines.len() {
            inside[end] = true;
            let code = strip(Language::Rust, lines[end]);
            for character in code.chars() {
                match character {
                    '{' => {
                        depth += 1;
                        opened = true;
                    }
                    '}' => depth -= 1,
                    _ => {}
                }
            }
            if (opened && depth <= 0) || (!opened && end > index && code.trim_end().ends_with(';'))
            {
                break;
            }
            end += 1;
        }
        index = end + 1;
    }
    inside
}

/// The line with its string literals emptied and its trailing comment removed, so a pattern
/// inside a string or a comment is not a finding.
fn strip(language: Language, line: &str) -> String {
    let comment = match language {
        Language::Python => "#",
        Language::Rust | Language::TypeScript => "//",
    };
    let mut out = String::with_capacity(line.len());
    let mut chars = line.char_indices().peekable();
    while let Some((at, character)) = chars.next() {
        if line[at..].starts_with(comment) {
            break;
        }
        let quote = match character {
            '"' => Some('"'),
            '`' if language == Language::TypeScript => Some('`'),
            '\'' if language != Language::Rust => Some('\''),
            // A Rust char literal is exactly one (possibly escaped) character; `'a` is a lifetime.
            '\'' => {
                let rest = &line[at + 1..];
                let closes = rest.starts_with('\\') && rest.chars().nth(2) == Some('\'')
                    || rest.chars().nth(1) == Some('\'');
                closes.then_some('\'')
            }
            _ => None,
        };
        let Some(quote) = quote else {
            out.push(character);
            continue;
        };
        out.push(quote);
        let mut escaped = false;
        for (_, inner) in chars.by_ref() {
            if escaped {
                escaped = false;
            } else if inner == '\\' {
                escaped = true;
            } else if inner == quote {
                break;
            }
        }
        out.push(quote);
    }
    out
}

/// A few head lines around `index`, joined: enough for a call written over several lines.
fn window(lines: &[&str], index: usize) -> String {
    let start = index.saturating_sub(4);
    let end = (index + 4).min(lines.len());
    lines
        .get(start..end)
        .map_or_else(String::new, |slice| slice.join("\n"))
}

/// The rules an added production line breaks, each with the fix it should name.
fn rules(
    language: Language,
    code: &str,
    raw: &str,
    context: &str,
    lines: &[&str],
    index: usize,
) -> Vec<(&'static str, &'static str)> {
    let mut found = Vec::new();
    let trimmed = code.trim();
    match language {
        Language::Rust => {
            if code.contains(".unwrap()")
                || code.contains(".expect(")
                || ["panic!(", "todo!(", "unimplemented!("]
                    .iter()
                    .any(|word| has_word(code, word))
            {
                found.push((
                    NO_PANIC,
                    "return a Result with a code and a reason instead of panicking",
                ));
            }
            if trimmed.starts_with("let _ =")
                || trimmed.starts_with("let _=")
                || trimmed.ends_with(".ok();")
            {
                found.push((NO_SWALLOW, "handle the error or return it with its reason"));
            }
            if [
                ".output()",
                ".wait()",
                ".wait_with_output()",
                "reqwest::get(",
                "reqwest::blocking::get(",
                "ureq::get(",
                "ureq::post(",
            ]
            .iter()
            .any(|call| code.contains(call))
            {
                found.push((
                    DEADLINE,
                    "run the external call with a deadline (a bounded runner or a client timeout)",
                ));
            }
        }
        Language::TypeScript => {
            let thrown = raw.trim().strip_prefix("throw ").map(str::trim_start);
            if thrown.is_some_and(|value| {
                value.starts_with(['"', '\'', '`', '{'])
                    || value.starts_with(|c: char| c.is_ascii_digit())
                    || value.starts_with("null")
                    || value.starts_with("undefined")
            }) {
                found.push((
                    NO_PANIC,
                    "throw an Error with a message, or return a result the caller must read",
                ));
            }
            if empty_catch(code) {
                found.push((NO_SWALLOW, "handle the error or rethrow it with its reason"));
            }
            if has_word(code, "fetch(") && !context.contains("signal") {
                found.push((DEADLINE, "pass an AbortSignal with a timeout to fetch"));
            }
        }
        Language::Python => {
            if (trimmed == "raise" && !inside_except(lines, index, raw))
                || trimmed == "assert"
                || trimmed.starts_with("assert ")
            {
                found.push((
                    NO_PANIC,
                    "raise a named error with its reason; assert is stripped under -O",
                ));
            }
            let previous_except = index
                .checked_sub(1)
                .and_then(|above| lines.get(above))
                .is_some_and(|line| line.trim_start().starts_with("except"));
            if (trimmed.starts_with("except") && trimmed.ends_with(": pass"))
                || (trimmed == "pass" && previous_except)
            {
                found.push((
                    NO_SWALLOW,
                    "handle the exception or let it travel with its reason",
                ));
            }
            if [
                "subprocess.run(",
                "subprocess.call(",
                "subprocess.check_call(",
                "subprocess.check_output(",
                ".communicate(",
                ".wait()",
                "urlopen(",
                "requests.get(",
                "requests.post(",
                "requests.put(",
                "requests.delete(",
                "requests.request(",
            ]
            .iter()
            .any(|call| code.contains(call))
                && !context.contains("timeout")
            {
                found.push((DEADLINE, "pass timeout= to the call"));
            }
        }
    }
    if let Some(name) = declared_name(language, code)
        && banned_name(&name)
    {
        found.push((
            NAMES,
            "name the thing after what it does, not Manager, Util, Helper, Service, Common or Misc",
        ));
    }
    if bare_flag(language, code) {
        found.push((
            NO_BARE_FLAG,
            "pass an enum, a named argument, or call one of two named functions",
        ));
    }
    found
}

/// `word` at a position not preceded by an identifier character.
fn has_word(code: &str, word: &str) -> bool {
    code.match_indices(word).any(|(at, _)| {
        code[..at]
            .chars()
            .next_back()
            .is_none_or(|before| !(before.is_alphanumeric() || before == '_' || before == '.'))
            || word.starts_with('.')
    })
}

fn empty_catch(code: &str) -> bool {
    let compact: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.contains("catch{}")
        || compact.contains(".catch(()=>{})")
        || compact.contains(".catch(()=>undefined)")
    {
        return true;
    }
    // `catch (error) {}`
    compact.match_indices("catch(").any(|(at, _)| {
        compact[at..]
            .find(')')
            .is_some_and(|close| compact[at + close + 1..].starts_with("{}"))
    })
}

/// Whether a bare `raise` at `index` sits inside an `except` block, read from the head text.
fn inside_except(lines: &[&str], index: usize, raw: &str) -> bool {
    let indent = raw.len() - raw.trim_start().len();
    lines
        .get(..index)
        .unwrap_or(&[])
        .iter()
        .rev()
        .find_map(|line| {
            let depth = line.len() - line.trim_start().len();
            (!line.trim().is_empty() && depth < indent)
                .then(|| line.trim_start().starts_with("except"))
        })
        == Some(true)
}

fn declared_name(language: Language, code: &str) -> Option<String> {
    let mut rest = code.trim_start();
    let prefixes: &[&str] = match language {
        Language::Rust => &[
            "pub(crate) ",
            "pub(super) ",
            "pub ",
            "async ",
            "const ",
            "unsafe ",
        ],
        Language::TypeScript => &["export ", "default ", "declare ", "abstract ", "async "],
        Language::Python => &["async "],
    };
    loop {
        let before = rest;
        for prefix in prefixes {
            rest = rest.strip_prefix(prefix).unwrap_or(rest);
        }
        if rest == before {
            break;
        }
    }
    let keywords: &[&str] = match language {
        Language::Rust => &[
            "fn ", "struct ", "enum ", "trait ", "type ", "mod ", "union ",
        ],
        Language::TypeScript => &[
            "function* ",
            "function ",
            "class ",
            "interface ",
            "type ",
            "enum ",
        ],
        Language::Python => &["def ", "class "],
    };
    let after = keywords
        .iter()
        .find_map(|keyword| rest.strip_prefix(keyword))?;
    let name: String = after
        .trim_start()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// A name containing one of the vague words as a whole word: `StringUtils`, `run_helper`.
fn banned_name(name: &str) -> bool {
    const BANNED: [&str; 7] = [
        "manager", "util", "utils", "helper", "service", "common", "misc",
    ];
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut previous_lower = false;
    for character in name.chars() {
        if character == '_' || character == '$' {
            words.push(std::mem::take(&mut current));
            previous_lower = false;
            continue;
        }
        if character.is_uppercase() && previous_lower {
            words.push(std::mem::take(&mut current));
        }
        previous_lower = character.is_lowercase() || character.is_ascii_digit();
        current.push(character.to_ascii_lowercase());
    }
    words.push(current);
    words.iter().any(|word| BANNED.contains(&word.as_str()))
}

/// A call that passes a bare `true`, `false` (or Python's `None`) as a positional argument.
fn bare_flag(language: Language, code: &str) -> bool {
    let flags: &[&str] = match language {
        Language::Python => &["True", "False", "None"],
        Language::Rust | Language::TypeScript => &["true", "false"],
    };
    let bytes = code.as_bytes();
    for flag in flags {
        for (at, _) in code.match_indices(flag) {
            let before = code[..at].trim_end();
            let after = code[at + flag.len()..].trim_start();
            if !(before.ends_with('(') || before.ends_with(','))
                || !(after.starts_with(',') || after.starts_with(')'))
            {
                continue;
            }
            // The open parenthesis of this argument list, and the callee written before it.
            let mut depth = 0_i32;
            let mut open = None;
            for (position, byte) in bytes[..at].iter().enumerate().rev() {
                match byte {
                    b')' | b']' | b'}' => depth += 1,
                    b'(' | b'[' | b'{' if depth > 0 => depth -= 1,
                    b'(' => {
                        open = Some(position);
                        break;
                    }
                    b'[' | b'{' => break,
                    _ => {}
                }
            }
            let Some(open) = open else {
                continue;
            };
            let callee: String = code[..open]
                .trim_end()
                .chars()
                .rev()
                .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '$' | '.' | ':' | '!'))
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if !exempt_callee(language, &callee) {
                return true;
            }
        }
    }
    false
}

/// Calls where a bare boolean is the value itself, not a mode switch: constructors, assertions,
/// state setters, standard containers and the combinators whose argument is a default value
/// (`unwrap_or(false)` supplies a value; it switches no mode).
fn exempt_callee(language: Language, callee: &str) -> bool {
    let last = callee.rsplit(['.', ':']).next().unwrap_or(callee);
    if callee.is_empty() || last.is_empty() || callee.ends_with('!') {
        return true;
    }
    if language == Language::Rust && last.starts_with(|c: char| c.is_uppercase()) {
        return true;
    }
    const EXEMPT: [&str; 26] = [
        "unwrap_or",
        "map_or",
        "then_some",
        "or_insert",
        "Some",
        "Ok",
        "Err",
        "Boolean",
        "bool",
        "useState",
        "useRef",
        "isinstance",
        "print",
        "expect",
        "toBe",
        "toEqual",
        "get",
        "pop",
        "setdefault",
        "getattr",
        "dict",
        "new",
        "from",
        "push",
        "insert",
        "store",
    ];
    EXEMPT.contains(&last)
        || (last.starts_with("set") && last[3..].starts_with(|c: char| c.is_uppercase()))
}

enum Excuse {
    Honoured { reason: String, until: String },
    Expired(String),
    None,
}

/// `keel-allow: <rule> <reason> until <YYYY-MM-DD|#issue>` on the line or the line above.
fn excuse(raw: &str, above: Option<&str>, rule: &str, today: &str) -> Excuse {
    for line in [Some(raw), above].into_iter().flatten() {
        let Some((_, allow)) = line.split_once("keel-allow:") else {
            continue;
        };
        let mut words = allow.trim().splitn(2, char::is_whitespace);
        if words.next() != Some(rule) {
            continue;
        }
        let rest = words.next().unwrap_or("");
        let Some((reason, until)) = rest.rsplit_once(" until ") else {
            continue;
        };
        let reason = reason.trim();
        let until = until
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches(['*', '/'])
            .to_owned();
        let issue = until
            .strip_prefix('#')
            .is_some_and(|number| !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()));
        let date = until.len() == 10
            && until.chars().enumerate().all(|(at, c)| {
                if at == 4 || at == 7 {
                    c == '-'
                } else {
                    c.is_ascii_digit()
                }
            });
        if reason.is_empty() || !(issue || date) {
            continue;
        }
        if date && until.as_str() < today {
            return Excuse::Expired(until);
        }
        return Excuse::Honoured {
            reason: reason.to_owned(),
            until,
        };
    }
    Excuse::None
}
