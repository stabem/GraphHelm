//! Bounded bridge to the official Codex app-server JSON-RPC interface.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const MAX_FRAME: usize = 1024 * 1024;
const MAX_STDERR: usize = 64 * 1024;
const MAX_MESSAGE: usize = 2000;
const MAX_TITLE: usize = 512;
const MAX_PATH: usize = 4096;
const IO_TIMEOUT: Duration = Duration::from_secs(30);
const TURN_TIMEOUT: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_millis(20);

/// Trusted server-side configuration for the native Codex executable.
pub struct NativeChatConfig {
    pub program: PathBuf,
    pub sqlite_home: Option<PathBuf>,
}

pub fn list(config: &NativeChatConfig, cursor: Option<&str>) -> Result<Value, String> {
    if cursor.is_some_and(|value| value.is_empty() || value.len() > 4096) {
        return Err("native catalog cursor is invalid".into());
    }
    let mut rpc = Rpc::spawn(config)?;
    rpc.request(
        "initialize",
        json!({
            "clientInfo": {"name":"graphhelm","title":"GraphHelm","version":"0.1.0"},
            "capabilities": {"experimentalApi": true}
        }),
    )?;
    rpc.notify("initialized", json!({}))?;
    let result = rpc.request(
        "thread/list",
        json!({
            "limit": 50,
            "cursor": cursor,
            "sortKey": "updated_at",
            "useStateDbOnly": true,
            "sourceKinds": ["cli", "vscode", "appServer"]
        }),
    )?;
    rpc.finish()?;
    let rows = result
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| result.get("threads").and_then(Value::as_array))
        .ok_or_else(|| "thread/list returned no bounded data array".to_owned())?;
    let mut chats = Vec::with_capacity(rows.len().min(50));
    for row in rows.iter().take(50) {
        let id = string_field(row, &["id", "threadId"])?;
        if !valid_uuid(&id) {
            return Err("thread/list returned an invalid thread id".into());
        }
        let project_directory = row
            .get("cwd")
            .or_else(|| row.get("projectDirectory"))
            .and_then(Value::as_str)
            .ok_or_else(|| "thread/list returned an invalid project directory".to_owned())?;
        let updated_at = row
            .get("updatedAt")
            .or_else(|| row.get("updated_at"))
            .cloned()
            .filter(|v| v.is_i64() || v.is_u64())
            .ok_or_else(|| "thread/list returned an invalid updatedAt".to_owned())?;
        let title = row
            .get("name")
            .or_else(|| row.get("title"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or(&id);
        chats.push(json!({
            "id": id,
            "title": bounded_string(title, MAX_TITLE),
            "projectDirectory": bounded_string(project_directory, MAX_PATH),
            "updatedAt": updated_at
        }));
    }
    Ok(
        json!({"chats": chats, "nextCursor": result.get("nextCursor").cloned().unwrap_or(Value::Null)}),
    )
}

pub fn send<F>(
    config: &NativeChatConfig,
    thread_id: &str,
    message: &str,
    expected_source_directory: &Path,
    observe: F,
) -> Result<(), String>
where
    F: FnMut(Value) -> Result<(), String>,
{
    send_impl(
        config,
        thread_id,
        message,
        expected_source_directory,
        observe,
    )
}

fn send_impl<F>(
    config: &NativeChatConfig,
    thread_id: &str,
    message: &str,
    expected_source_directory: &Path,
    mut observe: F,
) -> Result<(), String>
where
    F: FnMut(Value) -> Result<(), String>,
{
    if !valid_uuid(thread_id) {
        return Err("thread id must be a stable UUID".into());
    }
    if message.is_empty() || message.chars().count() > MAX_MESSAGE {
        return Err("message must contain 1..2000 characters".into());
    }
    let mut rpc = Rpc::spawn(config)?;
    rpc.request(
        "initialize",
        json!({
            "clientInfo": {"name":"graphhelm","title":"GraphHelm","version":"0.1.0"},
            "capabilities": {"experimentalApi": true}
        }),
    )?;
    rpc.notify("initialized", json!({}))?;
    let metadata = rpc.request(
        "thread/read",
        json!({"threadId": thread_id, "includeTurns": false}),
    )?;
    let metadata = metadata.get("thread").unwrap_or(&metadata);
    let cwd = metadata
        .get("cwd")
        .or_else(|| metadata.get("projectDirectory"))
        .and_then(Value::as_str)
        .ok_or_else(|| "thread/read did not return a verified cwd".to_owned())?;
    if cwd.is_empty() || cwd.len() > MAX_PATH {
        return Err("thread cwd is invalid".into());
    }
    let expected = expected_source_directory
        .canonicalize()
        .map_err(|_| "selected source directory is unavailable".to_owned())?;
    let returned = Path::new(cwd)
        .canonicalize()
        .map_err(|_| "thread source directory is unavailable".to_owned())?;
    if expected != returned {
        return Err("thread source directory does not match the selected project".into());
    }
    let source_id = metadata
        .get("id")
        .or_else(|| metadata.get("threadId"))
        .and_then(Value::as_str)
        .unwrap_or(thread_id);
    if source_id != thread_id || !valid_uuid(source_id) {
        return Err("thread/read returned a mismatched id".into());
    }
    let title = metadata
        .get("name")
        .or_else(|| metadata.get("title"))
        .and_then(Value::as_str)
        .map(|s| bounded_string(s, MAX_TITLE))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| bounded_string(thread_id, MAX_TITLE));
    // Deliberately pass no model, provider, sandbox, approval, or other setting: server metadata owns them.
    rpc.request(
        "thread/resume",
        json!({"threadId": thread_id, "excludeTurns": true}),
    )?;
    let turn_result = rpc.request(
        "turn/start",
        json!({"threadId": thread_id, "input": [{"type":"text","text":message}]}),
    )?;
    let turn_id: Option<String> = turn_result
        .pointer("/turn/id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let turn_id = turn_id.ok_or_else(|| "Codex turn/start returned no turn id".to_owned())?;
    let mut final_text = String::new();
    loop {
        let event = rpc.event()?;
        if let Some(method) = event.get("method").and_then(Value::as_str) {
            if method == "turn/started" {
                if turn_event_matches(&event, "turn/started", thread_id, Some(&turn_id)) {
                    observe(
                        json!({"phase":"received","threadId":thread_id,"sourceId":source_id,"sourceCwd":cwd,"sourceDirectory":cwd,"title":title,"turnId":turn_id}),
                    )?;
                }
            } else if method == "turn/completed" {
                if turn_event_matches(&event, "turn/completed", thread_id, Some(&turn_id)) {
                    if event.pointer("/params/turn/status").and_then(Value::as_str)
                        != Some("completed")
                    {
                        return Err("Codex turn did not complete".into());
                    }
                    let mut out = json!({"phase":"completed","threadId":thread_id,"sourceId":source_id,"sourceCwd":cwd,"sourceDirectory":cwd,"title":title,"turnId":turn_id});
                    if !final_text.is_empty() {
                        out["finalText"] =
                            Value::String(final_text.chars().take(MAX_MESSAGE).collect());
                    }
                    observe(out)?;
                    rpc.finish()?;
                    return Ok(());
                }
            } else if method == "item/completed" {
                if turn_event_matches(&event, "item/completed", thread_id, Some(&turn_id)) {
                    collect_text(&event, &mut final_text);
                }
            } else if method.ends_with("/requestApproval")
                || method.ends_with("/requestElicitation")
                || method == "item/requestApproval"
            {
                return Err("blocked: server approval or elicitation request".into());
            }
        }
    }
}

struct Rpc {
    child: Child,
    group: graphhelm_process_tree::ProcessGroup,
    writer: Option<std::sync::mpsc::SyncSender<Vec<u8>>>,
    rx: std::sync::mpsc::Receiver<Result<Vec<u8>, String>>,
    pending: Vec<Value>,
    deadline: Instant,
}

impl Drop for Rpc {
    fn drop(&mut self) {
        if !graphhelm_process_tree::leader_exited(&mut self.child).unwrap_or(false) {
            let _ = graphhelm_process_tree::terminate(
                self.child.id(),
                graphhelm_process_tree::for_thread(self.group),
            );
            let _ = self.child.kill();
        }
        graphhelm_process_tree::close(&mut self.group);
        let _ = self.child.wait();
    }
}

impl Rpc {
    fn spawn(config: &NativeChatConfig) -> Result<Self, String> {
        let program = config
            .program
            .canonicalize()
            .map_err(|_| "configured Codex program is unavailable".to_owned())?;
        if !program.is_file() {
            return Err("configured Codex program is not a file".into());
        }
        let mut cmd = Command::new(program);
        if let Some(home) = &config.sqlite_home {
            let home = home
                .canonicalize()
                .map_err(|_| "configured sqlite home is unavailable".to_owned())?;
            if !home.is_dir() {
                return Err("configured sqlite home is not a directory".into());
            }
            let escaped = serde_json::to_string(&home.to_string_lossy().to_string())
                .map_err(|_| "configured sqlite home is invalid".to_owned())?;
            let value = format!("sqlite_home={escaped}");
            cmd.args(["-c", &value]);
        }
        cmd.arg("app-server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, _) in std::env::vars_os() {
            let name = key.to_string_lossy().to_ascii_uppercase();
            if name.starts_with("GRAPHHELM_")
                && (name.contains("KEY") || name.contains("TOKEN") || name.contains("CREDENTIAL"))
            {
                cmd.env_remove(key);
            }
        }
        graphhelm_process_tree::configure(&mut cmd);
        let mut child = cmd
            .spawn()
            .map_err(|_| "Codex app-server could not be started".to_owned())?;
        let group = match graphhelm_process_tree::create(&child) {
            Ok(group) => group,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Codex process containment unavailable".into());
            }
        };
        let stdout = match child.stdout.take() {
            Some(v) => v,
            None => {
                let _ = graphhelm_process_tree::terminate(
                    child.id(),
                    graphhelm_process_tree::for_thread(group),
                );
                let _ = child.kill();
                let _ = child.wait();
                return Err("Codex stdout unavailable".into());
            }
        };
        let stderr = match child.stderr.take() {
            Some(v) => v,
            None => {
                let _ = graphhelm_process_tree::terminate(
                    child.id(),
                    graphhelm_process_tree::for_thread(group),
                );
                let _ = child.kill();
                let _ = child.wait();
                return Err("Codex stderr unavailable".into());
            }
        };
        let (tx, rx) = std::sync::mpsc::sync_channel(64);
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = Vec::new();
                match reader
                    .by_ref()
                    .take((MAX_FRAME + 1) as u64)
                    .read_until(b'\n', &mut line)
                {
                    Ok(0) => {
                        let _ = tx.send(Err("Codex app-server closed stdout".into()));
                        break;
                    }
                    Ok(n) if n > MAX_FRAME || (n == MAX_FRAME + 1 && !line.ends_with(b"\n")) => {
                        let _ = tx.send(Err("Codex response frame exceeded limit".into()));
                        break;
                    }
                    Ok(_) => {
                        let _ = tx.send(
                            serde_json::from_slice::<Value>(&line)
                                .map(line_for)
                                .map_err(|_| "Codex sent malformed JSON".into()),
                        );
                    }
                    Err(_) => {
                        let _ = tx.send(Err("Codex stdout could not be read".into()));
                        break;
                    }
                }
            }
        });
        thread::spawn(move || {
            let mut r = BufReader::new(stderr);
            let mut b = vec![0; MAX_STDERR];
            let _ = r.read(&mut b);
        });
        let stdin = match child.stdin.take() {
            Some(v) => v,
            None => {
                let _ = graphhelm_process_tree::terminate(
                    child.id(),
                    graphhelm_process_tree::for_thread(group),
                );
                let _ = child.kill();
                let _ = child.wait();
                return Err("Codex stdin unavailable".into());
            }
        };
        let (writer, writer_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(4);
        thread::spawn(move || {
            let mut stdin = stdin;
            while let Ok(bytes) = writer_rx.recv() {
                if stdin.write_all(&bytes).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            group,
            writer: Some(writer),
            rx,
            pending: Vec::new(),
            deadline: Instant::now() + TURN_TIMEOUT,
        })
    }
    fn write(&mut self, value: Value) -> Result<(), String> {
        let bytes = serde_json::to_vec(&value).map_err(|_| "request encoding failed".to_owned())?;
        if bytes.len() > MAX_FRAME {
            return Err("request frame exceeded limit".into());
        }
        let mut frame = bytes;
        frame.push(b'\n');
        let remaining = self.remaining()?;
        let deadline = Instant::now() + remaining;
        let writer = self
            .writer
            .as_ref()
            .ok_or_else(|| "Codex request pipe closed".to_owned())?;
        loop {
            match writer.try_send(frame) {
                Ok(()) => break Ok(()),
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    break Err("Codex request pipe closed".into());
                }
                Err(std::sync::mpsc::TrySendError::Full(bytes)) => {
                    frame = bytes;
                    if Instant::now() >= deadline {
                        break Err("Codex request pipe timed out".into());
                    }
                    thread::sleep(POLL);
                }
            }
        }
    }
    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.write(json!({"method":method,"params":params}))
    }
    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = next_id();
        self.write(json!({"id":id,"method":method,"params":params}))?;
        loop {
            let v = self.recv_raw()?;
            if v.get("method").is_some() {
                if v.get("id").is_some() || is_blocking_request(&v) {
                    return Err("blocked: server approval or elicitation request".into());
                }
                if self.pending.len() >= 64 {
                    return Err("Codex notification queue exceeded limit".into());
                }
                self.pending.push(v);
                continue;
            }
            if v.get("id") == Some(&Value::Number(id.into())) {
                if v.get("error").is_some() {
                    return Err(format!("Codex request {method} failed: server error"));
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }
    fn event(&mut self) -> Result<Value, String> {
        self.next()
    }
    fn next(&mut self) -> Result<Value, String> {
        if !self.pending.is_empty() {
            return Ok(self.pending.remove(0));
        }
        self.recv_raw()
    }
    fn recv_raw(&mut self) -> Result<Value, String> {
        self.rx
            .recv_timeout(self.remaining()?.min(IO_TIMEOUT))
            .map_err(|_| "Codex response timed out or stream failed".to_owned())?
            .map(|line| {
                serde_json::from_slice(&line).map_err(|_| "Codex sent malformed JSON".into())
            })?
    }
    fn remaining(&self) -> Result<Duration, String> {
        self.deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| "Codex turn timed out".into())
    }
    fn finish(mut self) -> Result<(), String> {
        drop(self.writer.take());
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if graphhelm_process_tree::leader_exited(&mut self.child).unwrap_or(false) {
                graphhelm_process_tree::close(&mut self.group);
                let _ = self.child.wait();
                return Ok(());
            }
            thread::sleep(POLL);
        }
        let _ = graphhelm_process_tree::terminate(
            self.child.id(),
            graphhelm_process_tree::for_thread(self.group),
        );
        graphhelm_process_tree::close(&mut self.group);
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
}

fn next_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static ID: AtomicU64 = AtomicU64::new(1);
    ID.fetch_add(1, Ordering::Relaxed)
}
fn line_for(v: Value) -> Vec<u8> {
    serde_json::to_vec(&v).unwrap_or_default()
}
fn string_field(v: &Value, keys: &[&str]) -> Result<String, String> {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str))
        .map(str::to_owned)
        .ok_or_else(|| "thread/list row missing id".into())
}
fn bounded_string(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}
fn valid_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && [8, 13, 18, 23].iter().all(|&i| b[i] == b'-')
        && b.iter()
            .enumerate()
            .all(|(i, c)| [8, 13, 18, 23].contains(&i) || c.is_ascii_hexdigit())
}
fn is_blocking_request(v: &Value) -> bool {
    v.get("method").and_then(Value::as_str).is_some_and(|m| {
        m.contains("approval")
            || m.contains("Approval")
            || m.contains("elicitation")
            || m.contains("Elicitation")
    })
}
fn event_ids(v: &Value) -> (Option<String>, Option<String>) {
    let p = v.get("params").unwrap_or(v);
    (
        p.pointer("/turn/id")
            .or_else(|| p.get("turnId"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        p.get("threadId").and_then(Value::as_str).map(str::to_owned),
    )
}
fn turn_event_matches(v: &Value, method: &str, thread_id: &str, turn_id: Option<&str>) -> bool {
    if v.get("method").and_then(Value::as_str) != Some(method) {
        return false;
    }
    let (id, tid) = event_ids(v);
    tid.as_deref() == Some(thread_id) && id.as_deref() == turn_id
}
fn collect_text(v: &Value, out: &mut String) {
    let Some(item) = v.pointer("/params/item") else {
        return;
    };
    if item.get("type").and_then(Value::as_str) != Some("agentMessage") {
        return;
    }
    let Some(text) = item.get("text").and_then(Value::as_str) else {
        return;
    };
    match item.get("phase").and_then(Value::as_str) {
        // Providers may omit phase. Keep their latest completed assistant message
        // for compatibility; commentary and tool text never consume reply space.
        Some("final_answer") | None => *out = bounded_string(text, MAX_MESSAGE),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Protects final reply selection from commentary exhausting the character budget.
    // Existing identity tests do not observe item content. Cost: pure JSON, no host or I/O.
    #[test]
    fn final_reply_survives_long_commentary_and_ignores_tool_text() {
        let mut reply = String::new();
        collect_text(
            &json!({"params":{"item":{"type":"agentMessage","phase":"commentary","text":"progress".repeat(MAX_MESSAGE)}}}),
            &mut reply,
        );
        assert!(reply.is_empty());
        collect_text(
            &json!({"params":{"item":{"type":"agentMessage","phase":null,"text":"legacy reply"}}}),
            &mut reply,
        );
        assert_eq!(reply, "legacy reply");
        collect_text(
            &json!({"params":{"item":{"type":"agentMessage","phase":"final_answer","text":"é".repeat(MAX_MESSAGE + 1)}}}),
            &mut reply,
        );
        assert_eq!(reply, "é".repeat(MAX_MESSAGE));
        collect_text(
            &json!({"params":{"item":{"type":"commandExecution","text":"tool output"}}}),
            &mut reply,
        );
        assert_eq!(reply, "é".repeat(MAX_MESSAGE));
    }
    #[test]
    fn uuid_validation_rejects_unstable_ids() {
        assert!(valid_uuid("019fdfe7-b5fa-7ca1-89c8-9651ad856819"));
        assert!(!valid_uuid("thread-1"));
    }
    #[test]
    fn response_matching_does_not_consume_reserved_server_request() {
        let v = json!({"id":1,"method":"item/requestApproval","params":{}});
        assert!(is_blocking_request(&v));
    }
    #[test]
    fn turn_matching_rejects_other_turns_and_matches_item_wire_identity() {
        let other = json!({"method":"turn/completed","params":{"threadId":"t","turn":{"id":"other","status":"completed"}}});
        assert!(!turn_event_matches(
            &other,
            "turn/completed",
            "t",
            Some("actual")
        ));
        let item = json!({"method":"item/completed","params":{"threadId":"t","turnId":"actual","item":{"type":"agentMessage","text":"reply"}}});
        assert!(turn_event_matches(
            &item,
            "item/completed",
            "t",
            Some("actual")
        ));
    }
}
