//! #1150: a human summary on STDERR for the three commands a stranger runs first.
//!
//! The owner ran `gateway setup`, the command whose whole premise is "the operator fills in only
//! the key", and got 1.5 KB of JSON on one line as the entire answer. It had worked — route
//! created, key sealed, probe green — and the person who typed it had to find that out by reading
//! a machine document.
//!
//! `AGENTS.md:49` keeps `apps/cli` at JSON presentation only and 47 test cells parse
//! `output.stdout` as JSON, so the answer is NOT prose on stdout. It is this: **stdout keeps the
//! contract byte for byte, and a summary goes to stderr when stdout is a terminal**. A pipe, a
//! test, the MCP tool and every harness see exactly what they saw before; a person at a prompt
//! sees what is now true.
//!
//! Three properties this rendering has, and they are the reason it exists rather than decoration:
//!
//! - it says WHAT IS NOW TRUE, which the JSON only implies across six sibling objects;
//! - it says WHAT TO RUN NEXT in the shell the operator is standing in — the `next` array already
//!   holds a `bash` and a `powershell` spelling of every step and buries both;
//! - it says WHAT THE COMMAND DID NOT DO: `gateway probe` places no model call, which is exactly
//!   the thing a person otherwise assumes was tested.
//!
//! **Every renderer reads only from `output.data`.** Nothing here re-derives a path, a state or a
//! verdict; if a fact is not in the envelope the CLI published, it is not printed. A renderer that
//! computed its own answer would be a second presentation layer able to disagree with the first.
//!
//! **It is field-selective, never a dump.** The setup envelope is the one that stands next to a
//! secret, and a renderer written as "serialize `data`" would print whatever a later change adds
//! to it. Each field printed here is named; `a_field_the_renderer_does_not_name_is_not_printed`
//! holds that line.
//!
//! [`render`] returns `None` for every command without a renderer, so adopting this cost the rest
//! of the CLI nothing.

use std::fmt::Write as _;

use serde_json::Value;

use crate::output::CommandOutput;

/// The human summary for this envelope, or `None` when this command has no renderer.
///
/// A failed outcome renders its diagnostics as plain lines instead of the success shape: a person
/// who typed the command wrong must see why, not a blob with `"ok":false` somewhere inside it.
pub fn render(output: &CommandOutput) -> Option<String> {
    let renderer: fn(&Value) -> String = match output.command {
        "gateway.setup" => setup,
        "gateway.probe" => probe,
        "init" => init,
        _ => return None,
    };
    if !output.ok {
        return Some(failure(output));
    }
    output.data.as_ref().map(renderer)
}

/// A refusal, as the person who typed the command needs it: the command that did not run, then
/// one line per diagnostic with the pointer that names where the input was wrong.
fn failure(output: &CommandOutput) -> String {
    let mut text = format!("{} did not run.\n\n", output.command);
    for diagnostic in &output.diagnostics {
        let _ = writeln!(
            text,
            "  {} ({} at {})",
            diagnostic.message, diagnostic.code, diagnostic.path
        );
    }
    text
}

/// One labelled line of the "what is now true" block. The padding is BUILT rather than spelled:
/// a run of three or more spaces inside a string literal is what `source_invariants.rs` refuses,
/// because that is the shape a lost continuation escape leaves behind.
fn field(label: &str, value: &str) -> String {
    format!("  {label:<10}{value}")
}

/// A string field, or a placeholder. Every caller names a field the command's own `execute` puts
/// in `data`; the placeholder is what a future envelope that dropped one would show, rather than
/// a panic or a silently empty line.
fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("(not reported)")
}

/// The `next` block, with the spelling for the shell this binary was built for. The `step` is the
/// explanation above its own command, which is where the JSON buries it.
fn next_steps(data: &Value, out: &mut String) {
    let Some(entries) = data.get("next").and_then(Value::as_array) else {
        return;
    };
    if entries.is_empty() {
        return;
    }
    out.push_str("\nNext, in this shell:\n");
    let key = if cfg!(windows) { "powershell" } else { "bash" };
    for entry in entries {
        let _ = writeln!(out, "  # {}", text(&entry["step"]));
        let _ = writeln!(out, "  {}", text(&entry[key]));
    }
}

/// `gateway setup`, the envelope that raised #1150. The key line is the load-bearing one: the
/// operator pasted a secret and has to know where it went and that it went nowhere readable.
fn setup(data: &Value) -> String {
    let route = &data["route"];
    let mut out = format!("Route '{}' is ready.\n\n", text(&route["id"]));
    let provider = format!(
        "{} ({} at {})",
        text(&route["provider"]),
        text(&route["model"]),
        text(&route["baseUrl"])
    );
    let _ = writeln!(out, "{}", field("provider", &provider));
    let key = format!(
        "sealed in {} as {}, and in no file you can read",
        text(&data["credential"]["broker"]),
        text(&route["credentialRef"])
    );
    let _ = writeln!(out, "{}", field("key", &key));
    let manifest = &data["manifest"];
    let manifest_line = format!("{} ({})", text(&manifest["path"]), text(&manifest["state"]));
    let _ = writeln!(out, "{}", field("manifest", &manifest_line));
    let _ = writeln!(out, "{}", field("probe", &probe_line(&data["probe"])));
    next_steps(data, &mut out);
    out
}

/// The probe's own reply as a sentence, INCLUDING what it did not do. A green probe proves the
/// credential leases from the broker; it places no model call, so a person who reads "available"
/// and concludes the provider answered has concluded something nobody measured.
fn probe_line(probe: &Value) -> String {
    let checks = probe
        .get("checks")
        .and_then(Value::as_array)
        .map(|checks| {
            checks
                .iter()
                .map(|check| {
                    let verdict = if check["ok"].as_bool() == Some(true) {
                        "ok"
                    } else {
                        "FAILED"
                    };
                    format!("{} {verdict}", text(&check["name"]))
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    format!(
        "{} ({checks}); it placed no model call, so it spent nothing",
        text(&probe["health"])
    )
}

/// `gateway probe` standalone. Its envelope names no next step, so none is invented.
fn probe(data: &Value) -> String {
    let mut out = format!("Route '{}' probed.\n\n", text(&data["route"]));
    let _ = writeln!(out, "{}", field("health", &probe_line(data)));
    out
}

/// `init`. What the project now has, then the commands that take the operator from an empty
/// directory to a running execution.
fn init(data: &Value) -> String {
    let mut out = format!("Project '{}' is initialized.\n\n", text(&data["project"]));
    let runtime = format!("{} (bind {})", text(&data["root"]), text(&data["bind"]));
    let _ = writeln!(out, "{}", field("runtime", &runtime));
    for (label, artifact) in [("events", &data["events"]), ("token", &data["token"])] {
        let line = format!("{} ({})", text(&artifact["path"]), text(&artifact["state"]));
        let _ = writeln!(out, "{}", field(label, &line));
    }
    let key = &data["key"];
    let key_line = format!(
        "{} ({}), read from {}",
        text(&key["path"]),
        text(&key["state"]),
        text(&key["environment"])
    );
    let _ = writeln!(out, "{}", field("key", &key_line));
    let keyring = &data["keyring"];
    let keyring_line = format!(
        "{} ({}, key id {})",
        text(&keyring["path"]),
        text(&keyring["state"]),
        text(&keyring["keyId"])
    );
    let _ = writeln!(out, "{}", field("keyring", &keyring_line));
    if let Some(harnesses) = data.get("harnesses").and_then(Value::as_array)
        && !harnesses.is_empty()
    {
        let registered = harnesses
            .iter()
            .map(|entry| format!("{} in {}", text(&entry["harness"]), text(&entry["path"])))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "{}", field("harnesses", &registered));
    }
    next_steps(data, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    use graphhelm_protocols::Diagnostic;
    use serde_json::json;

    use crate::error_codes::GHCLI009_GATEWAY_INVALID;

    /// The setup envelope this file renders, with the fields `setup::execute` actually publishes.
    /// `apiKey` is NOT one of them: it is planted here as the field a future envelope might carry,
    /// so `a_field_the_renderer_does_not_name_is_not_printed` can fail a renderer that dumps.
    fn setup_data() -> Value {
        json!({
            "manifest": {"path": ".graphhelm/manifest.json", "state": "created"},
            "route": {
                "id": "judge",
                "provider": "typesafe",
                "model": "jev-latest",
                "baseUrl": "https://api.typesafe.ai",
                "credentialRef": "secret_typesafe",
            },
            "credential": {"broker": ".graphhelm/broker", "usableBy": ["judge"]},
            "probe": {
                "route": "judge",
                "checks": [{"name": "credential", "ok": true}],
                "health": "available",
            },
            "apiKey": "sk-SENTINEL-1150-must-never-render",
            "next": [{
                "step": "export the broker passphrase from serve.key",
                "powershell": "$env:GRAPHHELM_GATEWAY_KEY = (Get-Content -Raw 'C:\\p\\serve.key').Trim()",
                "bash": "export GRAPHHELM_GATEWAY_KEY=\"$(cat /p/serve.key)\"",
            }],
        })
    }

    fn success(command: &'static str, data: Value) -> CommandOutput {
        CommandOutput {
            ok: true,
            command,
            data: Some(data),
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn a_command_without_a_renderer_renders_nothing() {
        for command in ["schema", "graph.synthesize", "execution.start", "gateway"] {
            assert!(
                render(&success(command, json!({"anything": true}))).is_none(),
                "{command}"
            );
        }
        // The failure shape is scoped the same way: a command with no renderer stays silent even
        // when it refuses, so adopting this changed nothing outside the three.
        assert!(
            render(&CommandOutput {
                ok: false,
                command: "schema",
                data: None,
                diagnostics: vec![Diagnostic::error("GHX", "no", "/x", "schema")],
            })
            .is_none()
        );
    }

    #[test]
    fn a_failed_outcome_renders_its_diagnostic_messages() {
        let output = CommandOutput {
            ok: false,
            command: "gateway.setup",
            data: None,
            diagnostics: vec![Diagnostic::error(
                GHCLI009_GATEWAY_INVALID,
                "the API key must not be empty",
                "/stdin",
                "gateway-cli",
            )],
        };
        let rendered = render(&output).expect("gateway.setup has a renderer");
        assert!(
            rendered.contains("gateway.setup did not run."),
            "{rendered}"
        );
        assert!(
            rendered.contains("the API key must not be empty"),
            "{rendered}"
        );
        assert!(rendered.contains("/stdin"), "{rendered}");
        assert!(rendered.contains(GHCLI009_GATEWAY_INVALID), "{rendered}");
        // Not the success shape: a refusal wired no route.
        assert!(!rendered.contains("is ready"), "{rendered}");
    }

    #[test]
    fn the_setup_rendering_names_the_route_the_manifest_state_and_the_absent_model_call() {
        let rendered = render(&success("gateway.setup", setup_data())).expect("rendered");
        assert!(
            rendered.starts_with("Route 'judge' is ready."),
            "{rendered}"
        );
        assert!(rendered.contains("typesafe"), "{rendered}");
        assert!(rendered.contains("jev-latest"), "{rendered}");
        assert!(rendered.contains("https://api.typesafe.ai"), "{rendered}");
        assert!(rendered.contains("secret_typesafe"), "{rendered}");
        assert!(rendered.contains(".graphhelm/broker"), "{rendered}");
        assert!(
            rendered.contains(".graphhelm/manifest.json (created)"),
            "{rendered}"
        );
        assert!(
            rendered.contains("it placed no model call"),
            "the sentence a person otherwise assumes away: {rendered}"
        );
        assert!(rendered.contains("Next, in this shell:"), "{rendered}");
        assert!(
            rendered.lines().count() <= 20,
            "the summary must stay readable: {rendered}"
        );
    }

    /// A renderer written as "serialize `data`" would pass every assertion above and print the
    /// next secret-adjacent field somebody adds to the envelope. The planted `apiKey` is never
    /// named by any renderer, so it must not appear; the route id, which IS named, must — a
    /// positive control, so a render that returned an empty string cannot pass this cell.
    #[test]
    fn a_field_the_renderer_does_not_name_is_not_printed() {
        let data = setup_data();
        // THE PLANT MUST BE THERE TO BE EXCLUDED (`graphhelm-pr-1099-agent-4a311e` on #1152).
        //
        // Without this the cell is VACUOUSLY green and measurably so: with `apiKey` deleted from
        // the fixture entirely it still passes, `control_found=true, sentinel_found=false` — so it
        // proved the renderer does not print `apiKey` and would prove exactly the same thing if
        // the renderer printed nothing at all, or if a later edit dropped the field from
        // `setup_data`. The absence below is a claim about the RENDERER only while the fixture is
        // known to carry the thing being looked for.
        assert_eq!(
            data["apiKey"], "sk-SENTINEL-1150-must-never-render",
            "the fixture must carry the planted secret or the absences below prove nothing"
        );
        let rendered = render(&success("gateway.setup", data)).expect("rendered");
        assert!(
            !rendered.contains("sk-SENTINEL-1150-must-never-render"),
            "{rendered}"
        );
        assert!(!rendered.contains("apiKey"), "{rendered}");
        assert!(rendered.contains("judge"), "control: {rendered}");
    }

    #[test]
    fn the_next_command_is_the_spelling_for_this_platform() {
        let rendered = render(&success("gateway.setup", setup_data())).expect("rendered");
        assert!(
            rendered.contains("export the broker passphrase from serve.key"),
            "the step explains the command above it: {rendered}"
        );
        if cfg!(windows) {
            assert!(
                rendered.contains("$env:GRAPHHELM_GATEWAY_KEY"),
                "{rendered}"
            );
            assert!(
                !rendered.contains("export GRAPHHELM_GATEWAY_KEY"),
                "{rendered}"
            );
        } else {
            assert!(
                rendered.contains("export GRAPHHELM_GATEWAY_KEY"),
                "{rendered}"
            );
            assert!(
                !rendered.contains("$env:GRAPHHELM_GATEWAY_KEY"),
                "{rendered}"
            );
        }
    }

    #[test]
    fn the_probe_rendering_names_the_route_the_health_and_what_it_did_not_do() {
        let data = json!({
            "route": "judge",
            "checks": [{"name": "credential", "ok": true}],
            "health": "available",
        });
        let rendered = render(&success("gateway.probe", data)).expect("rendered");
        assert!(rendered.contains("Route 'judge' probed."), "{rendered}");
        assert!(rendered.contains("available"), "{rendered}");
        assert!(rendered.contains("credential ok"), "{rendered}");
        assert!(rendered.contains("it placed no model call"), "{rendered}");
        // Nothing in a probe envelope names a next step, and none is invented.
        assert!(!rendered.contains("Next, in this shell:"), "{rendered}");
    }

    #[test]
    fn a_failed_check_is_not_rendered_as_ok() {
        let data = json!({
            "route": "judge",
            "checks": [{"name": "credential", "ok": false}],
            "health": "degraded",
        });
        let rendered = render(&success("gateway.probe", data)).expect("rendered");
        assert!(rendered.contains("credential FAILED"), "{rendered}");
        assert!(rendered.contains("degraded"), "{rendered}");
    }

    #[test]
    fn the_init_rendering_names_the_layout_and_the_next_steps() {
        let data = json!({
            "project": ".",
            "root": ".graphhelm",
            "bind": "127.0.0.1:8099",
            "events": {"path": ".graphhelm/events", "state": "created"},
            "token": {"path": ".graphhelm/events/serve.token", "state": "created"},
            "key": {
                "path": ".graphhelm/serve.key",
                "state": "created",
                "environment": "GRAPHHELM_SEALING_KEY",
            },
            "keyring": {
                "path": ".graphhelm/keyring",
                "state": "created",
                "keyId": "studio",
            },
            "harnesses": [{
                "harness": "claude-code",
                "path": ".mcp.json",
                "state": "created",
                "note": "restart the session",
            }],
            "next": [{
                "step": "start the Runtime on loopback",
                "powershell": "graphhelm serve --bind 127.0.0.1:8099",
                "bash": "graphhelm serve --bind 127.0.0.1:8099",
            }],
        });
        let rendered = render(&success("init", data)).expect("rendered");
        assert!(
            rendered.starts_with("Project '.' is initialized."),
            "{rendered}"
        );
        assert!(rendered.contains("127.0.0.1:8099"), "{rendered}");
        assert!(rendered.contains(".graphhelm/serve.key"), "{rendered}");
        assert!(rendered.contains("GRAPHHELM_SEALING_KEY"), "{rendered}");
        assert!(rendered.contains("key id studio"), "{rendered}");
        assert!(rendered.contains("claude-code in .mcp.json"), "{rendered}");
        assert!(
            rendered.contains("start the Runtime on loopback"),
            "{rendered}"
        );
        assert!(rendered.lines().count() <= 20, "{rendered}");
    }
}
