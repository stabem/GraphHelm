//! #356: real browser/model-door privacy and deterministic draft observation.
//! Contract: only actual visits become a draft, and model input never contains a
//! declared password. Defect: leaking echoed text before model dispatch or inventing
//! unvisited edges. Replay/driver tests do not observe this production model boundary.
//! Cost: opt-in local Node/Playwright, Git, ~60s; no account, provider or network install.
//! The model fixture is real subprocess I/O, not a production executor seam.
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const SECRET: &str = "explore_password_canary_713840";
const MODEL: &str = r#"
const fs=require('node:fs');
let input=''; process.stdin.setEncoding('utf8');
process.stdin.on('data',chunk=>input+=chunk);
process.stdin.on('end',()=>{
  const match=/<observation>([\s\S]*)<\/observation>/.exec(input);
  if(!match)process.exit(1);
  const d=JSON.parse(match[1]), current=d.current, visited=d.visited;
  let proposal;
  if(current.includes('heading "Cart"')) proposal=visited.includes('cart /cart')?
    {act:{kind:'activate',role:'button',name:'Checkout'}}:{newScreen:{id:'cart',title:'Cart'}};
  else if(current.includes('heading "Checkout"')) {
    if(!visited.includes('pay /checkout'))proposal={newScreen:{id:'pay',title:'Checkout'}};
    else if(!d.recent.some(a=>a.secret==='password'))proposal={act:{kind:'enter_text',role:'textbox',name:'Password',secret:'password'}};
    else proposal={act:{kind:'submit',role:'button',name:'Submit order'}};
  } else if(current.includes('heading "Order 42"')) proposal=visited.includes('done /orders/:id')?
    {done:true}:{newScreen:{id:'done',title:'Order'}};
  else proposal={giveUp:'goal_unreachable'};
  const text=JSON.stringify(proposal);
  fs.appendFileSync(process.argv[2],JSON.stringify({prompt:input,reply:text})+'\n');
  process.stdout.write(JSON.stringify({subtype:'success',result:text,usage:{input_tokens:1,output_tokens:1}}));
});
"#;

struct App {
    child: Child,
    group: graphhelm_process_tree::ProcessGroup,
    base: String,
}
impl App {
    fn start(script: &Path) -> Self {
        let mut command = Command::new("node");
        command
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        graphhelm_process_tree::configure(&mut command);
        let mut child = command.spawn().expect("OBSERVER_MISSING: local Node app");
        let group = graphhelm_process_tree::create(&child).unwrap();
        let output = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(output).read_line(&mut line);
            let _ = tx.send(line);
        });
        let line = rx
            .recv_timeout(Duration::from_secs(30))
            .expect("OBSERVER_MISSING: app startup");
        let value: Value = serde_json::from_str(&line).unwrap();
        Self {
            child,
            group,
            base: value["base"].as_str().unwrap().to_owned(),
        }
    }
}
impl Drop for App {
    fn drop(&mut self) {
        let _ = graphhelm_process_tree::terminate(self.child.id(), self.group);
        graphhelm_process_tree::close(&mut self.group);
        let _ = self.child.wait();
    }
}
fn reply(command: &mut Command) -> (i32, Value) {
    let output = command.output().unwrap();
    let value = serde_json::from_slice(&output.stdout).expect("CLI JSON envelope");
    (output.status.code().unwrap(), value)
}
fn cli() -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command.arg("--json");
    command
}

#[test]
#[ignore = "requires explicitly installed local Playwright/Chromium observer"]
fn actual_exploration_redacts_model_input_and_repeats_the_same_draft() {
    let toolchain = std::env::var_os("GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT")
        .expect("OBSERVER_MISSING: GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT");
    let root = tempfile::tempdir().unwrap();
    let app_script = root.path().join("app.mjs");
    std::fs::write(
        &app_script,
        include_str!("../../../tools/journey-driver/fixture-server.mjs"),
    )
    .unwrap();
    let app = App::start(&app_script);
    let model = root.path().join("model.cjs");
    let transcript = root.path().join("transcript.jsonl");
    std::fs::write(&model, MODEL).unwrap();
    let manifest = root.path().join("routes.json");
    std::fs::write(&manifest,serde_json::to_vec(&json!({"manifestVersion":1,"routes":[{
        "id":"explore_observer","provider":"anthropic","transport":"native_runtime","runtime":"claude_code",
        "authentication":"account_subscription","billingMode":"subscription_quota","command":{"program":"node","args":[model,transcript]},
        "profiles":["software_execution"],"enabled":true,"timeoutSeconds":10}]})).unwrap()).unwrap();
    let mut drafts = Vec::new();
    for index in 0..2 {
        let project = root.path().join(format!("project-{index}"));
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("package.json"), "{\"private\":true}").unwrap();
        let linked=Command::new("node").args(["-e","require('node:fs').symlinkSync(process.argv[1],process.argv[2],process.platform==='win32'?'junction':'dir')"])
            .arg(Path::new(&toolchain).join("node_modules")).arg(project.join("node_modules")).status().unwrap();
        assert!(linked.success());
        let observer = project.join(".graphhelm/observers");
        std::fs::create_dir_all(&observer).unwrap();
        std::fs::write(
            observer.join("journey_driver.mjs"),
            include_bytes!("../../../tools/journey-driver/driver.mjs"),
        )
        .unwrap();
        let (code, value) = reply(
            cli()
                .args(["journey", "explore", "--id", "checkout", "--base"])
                .arg(format!("{}/cart", app.base))
                .args(["--goal", "Reach the order screen", "--project"])
                .arg(&project)
                .arg("--manifest")
                .arg(&manifest)
                .args(["--route", "explore_observer", "--secret", "password"])
                .env("GRAPHHELM_SECRET_password", SECRET),
        );
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["data"]["outcome"], "draft_completed");
        assert_eq!(value["data"]["screens"], 3);
        assert_eq!(value["data"]["edges"], 2);
        let draft =
            std::fs::read_to_string(project.join(".graphhelm/journeys/checkout.journey.yaml"))
                .unwrap();
        assert!(!draft.contains(SECRET));
        assert!(draft.contains("approved: null"));
        assert!(draft.contains("status: draft"));
        let flow: Value = serde_yaml_ng::from_str(&draft).unwrap();
        let edges = flow["edges"].as_array().unwrap();
        assert_eq!(edges.len(), 2);
        let pay = edges.iter().find(|e| e["from"] == "pay").unwrap();
        assert_eq!(
            pay["acts"],
            json!([{"kind":"enter_text","role":"textbox","name":"Password","secret":"password"},{"kind":"submit","role":"button","name":"Submit order"}])
        );
        assert!(
            flow["screens"]
                .as_array()
                .unwrap()
                .iter()
                .all(|s| s["scope"] == "unknown")
        );
        drafts.push(draft);
        let (code, value) = reply(
            cli()
                .args(["journey", "replay", "checkout", "--project"])
                .arg(&project)
                .env("GRAPHHELM_SECRET_password", SECRET),
        );
        assert_ne!(code, 0, "draft must not become approval: {value}");
        assert_eq!(value["data"]["modelCalls"], 0);
    }
    assert_eq!(drafts[0], drafts[1]);
    let transcript = std::fs::read_to_string(transcript).unwrap();
    assert!(!transcript.contains(SECRET));
    let turns: Vec<Value> = transcript
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(turns.len(), 14);
    assert!(
        turns
            .iter()
            .any(|v| v["prompt"].as_str().unwrap().contains("«secret:password»"))
    );
}

const DESTRUCTIVE_MODEL: &str = r#"
let input=''; process.stdin.setEncoding('utf8');
process.stdin.on('data',chunk=>input+=chunk);
process.stdin.on('end',()=>{
  const match=/<observation>([\s\S]*)<\/observation>/.exec(input);
  if(!match)process.exit(1);
  const d=JSON.parse(match[1]), current=d.current, visited=d.visited;
  let proposal;
  if(current.includes('heading "Account deleted"')) proposal=visited.includes('deleted /account/deleted')?
    {done:true}:{newScreen:{id:'deleted',title:'Deleted'}};
  else if(current.includes('heading "Account"')) proposal=visited.includes('account /account')?
    {act:{kind:'activate',role:'button',name:'Delete account'}}:{newScreen:{id:'account',title:'Account'}};
  else proposal={giveUp:'goal_unreachable'};
  process.stdout.write(JSON.stringify({subtype:'success',result:JSON.stringify(proposal),usage:{input_tokens:1,output_tokens:1}}));
});
"#;

/// The fixture app with its command channel kept open, so the test can read the server-side
/// effect counters after the CLI has run.
struct Fixture {
    app: App,
    lines: mpsc::Receiver<String>,
}
impl Fixture {
    fn start(script: &Path) -> Self {
        let mut command = Command::new("node");
        command
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        graphhelm_process_tree::configure(&mut command);
        let mut child = command.spawn().expect("OBSERVER_MISSING: local Node app");
        let group = graphhelm_process_tree::create(&child).unwrap();
        let output = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let first = lines
            .recv_timeout(Duration::from_secs(30))
            .expect("OBSERVER_MISSING: app startup");
        let value: Value = serde_json::from_str(&first).unwrap();
        let base = value["base"].as_str().unwrap().to_owned();
        Self {
            app: App { child, group, base },
            lines,
        }
    }
    fn deletes(&mut self) -> u64 {
        use std::io::Write;
        let stdin = self.app.child.stdin.as_mut().unwrap();
        stdin.write_all(b"counts\n").unwrap();
        stdin.flush().unwrap();
        let line = self.lines.recv_timeout(Duration::from_secs(10)).unwrap();
        serde_json::from_str::<Value>(&line).unwrap()["deletes"]
            .as_u64()
            .unwrap()
    }
}

fn observed_project(root: &Path, name: &str, toolchain: &std::ffi::OsStr) -> std::path::PathBuf {
    let project = root.join(name);
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("package.json"), "{\"private\":true}").unwrap();
    let linked = Command::new("node")
        .args(["-e", "require('node:fs').symlinkSync(process.argv[1],process.argv[2],process.platform==='win32'?'junction':'dir')"])
        .arg(Path::new(toolchain).join("node_modules"))
        .arg(project.join("node_modules"))
        .status()
        .unwrap();
    assert!(linked.success());
    let observer = project.join(".graphhelm/observers");
    std::fs::create_dir_all(&observer).unwrap();
    std::fs::write(
        observer.join("journey_driver.mjs"),
        include_bytes!("../../../tools/journey-driver/driver.mjs"),
    )
    .unwrap();
    project
}

// #356 Task 2: a deny-listed act must never reach the app, whatever the model proposes.
// Observer: the fixture server's own delete counter (a durable server-side effect), with
// a positive control in which an operator --allow-act lets the same act through.
#[test]
#[ignore = "requires explicitly installed local Playwright/Chromium observer"]
fn a_denied_act_never_reaches_the_app_and_an_allowed_one_does() {
    let toolchain = std::env::var_os("GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT")
        .expect("OBSERVER_MISSING: GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT");
    let root = tempfile::tempdir().unwrap();
    let app_script = root.path().join("app.mjs");
    std::fs::write(
        &app_script,
        include_str!("../../../tools/journey-driver/fixture-server.mjs"),
    )
    .unwrap();
    let mut fixture = Fixture::start(&app_script);
    let model = root.path().join("model.cjs");
    std::fs::write(&model, DESTRUCTIVE_MODEL).unwrap();
    let manifest = root.path().join("routes.json");
    std::fs::write(&manifest,serde_json::to_vec(&json!({"manifestVersion":1,"routes":[{
        "id":"explore_observer","provider":"anthropic","transport":"native_runtime","runtime":"claude_code",
        "authentication":"account_subscription","billingMode":"subscription_quota","command":{"program":"node","args":[model]},
        "profiles":["software_execution"],"enabled":true,"timeoutSeconds":10}]})).unwrap()).unwrap();
    let base = fixture.app.base.clone();
    let explore = |project: &Path, allow: &[&str]| {
        let mut command = cli();
        command
            .args(["journey", "explore", "--id", "account", "--base"])
            .arg(format!("{base}/account"))
            .args(["--goal", "Delete the account", "--project"])
            .arg(project)
            .arg("--manifest")
            .arg(&manifest)
            .args(["--route", "explore_observer"]);
        for expression in allow {
            command.args(["--allow-act", expression]);
        }
        reply(&mut command)
    };

    let denied = observed_project(root.path(), "denied", &toolchain);
    let (code, value) = explore(&denied, &[]);
    assert_eq!(code, 1, "{value}");
    assert_eq!(
        value["diagnostics"][0]["code"], "explore.action_denied",
        "{value}"
    );
    assert_eq!(value["data"]["acts"], 0, "{value}");
    assert_eq!(fixture.deletes(), 0, "a denied act reached the app");
    assert!(
        !denied
            .join(".graphhelm/journeys/account.journey.yaml")
            .exists()
    );

    let allowed = observed_project(root.path(), "allowed", &toolchain);
    let (code, value) = explore(&allowed, &["Delete account"]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["data"]["acts"], 1, "{value}");
    assert_eq!(
        fixture.deletes(),
        1,
        "the positive control did not reach the app"
    );
}
