//! `graphhelm gateway setup --provider <typesafe|anthropic|openai>` (#1139): one command that
//! wires a model provider into a project `init` provisioned, where the operator fills in only
//! the key.
//!
//! Before this command that was four hand steps across three documents: author a manifest route,
//! mint or reuse a keyring passphrase, `gateway credential set` with six flags reading the key
//! from stdin, `gateway probe`. `setup` does all of it against the layout `init` (#1062) made:
//!
//! - `<project>/.graphhelm/serve.key` is the passphrase and `<project>/.graphhelm/keyring` the
//!   keyring, both through `init`'s own [`init::ensure_sealing_keyring`] — created when absent,
//!   never rotated, reported `existing` on a second run.
//! - `<project>/.graphhelm/manifest.json` gains the provider's `direct_api` route (or is created
//!   around it). An existing route with the same id is replaced only with `--replace`; without it
//!   the run is refused BEFORE the key is asked for and the file is left byte-identical. The whole
//!   document passes [`RouteManifest::from_json`] before a byte is written, and the write is a
//!   temporary file renamed into place: a reader never sees half a manifest.
//! - The key is read ONCE, through [`read_key`]: a hidden prompt on stderr when stdin is a
//!   terminal (echo off through termios on Unix and the console mode on Windows), one trimmed
//!   line when stdin is a pipe. It is never an argument, never printed, never logged, and lands
//!   only in the Credential Broker (`<project>/.graphhelm/broker`) through the same
//!   [`CredentialBroker::store`] path `gateway credential set` uses, under the route's
//!   `credentialRef` and usable by that route alone.
//! - `gateway probe`'s own function runs on the new route, and `data.probe` carries its reply.
//! - `.graphhelm/` is asserted in `.gitignore` through `init`'s own helper.
//!
//! What `setup` never does: select a route for anything. It wires one; the caller names it on
//! every command that uses it (`--judge-route judge`, `--route <id>`). No automatic paid fallback.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use graphhelm_events::SecretBytes;
use graphhelm_gateway::manifest::RouteManifest;
use graphhelm_model_gateway::broker::{CredentialBroker, SecretReference};
use serde_json::{Map, Value, json};

use super::{
    Failure, ManifestReadError, broker_failure, credential_error, finish, invalid, probe,
    read_bounded_manifest, route, runtime,
};
use crate::args::{SetupArgs, SetupProvider};
use crate::commands::init::{
    self, GITIGNORE_FILE, KEY_FILE, KEYRING_DIRECTORY, RUNTIME_DIRECTORY, quoted_bash,
    quoted_powershell, validate_key_id,
};
use crate::output::Outcome;

const COMMAND: &str = "gateway.setup";
const MANIFEST_FILE: &str = "manifest.json";
const BROKER_DIRECTORY: &str = "broker";
const MANIFEST_VERSION: u64 = 1;
/// The one profile every provisioned route advertises. Scoring within a profile is deferred
/// (`core/gateway/src/manifest.rs`); the tag only has to be a legal member of the vocabulary.
const PROFILE: &str = "balanced_reasoning";

/// What `setup` knows about a provider: the defaults `--route-id`, `--model` and `--base-url`
/// override, and the name the route and the credential carry.
struct ProviderDefaults {
    name: &'static str,
    base_url: &'static str,
    route_id: &'static str,
    /// `None` for a provider that publishes no single model this command could pin without
    /// inventing one; `--model` is then required.
    model: Option<&'static str>,
}

const fn defaults(provider: SetupProvider) -> ProviderDefaults {
    match provider {
        SetupProvider::Typesafe => ProviderDefaults {
            name: "typesafe",
            base_url: "https://api.typesafe.ai",
            route_id: "judge",
            model: Some("jev-latest"),
        },
        SetupProvider::Anthropic => ProviderDefaults {
            name: "anthropic",
            base_url: "https://api.anthropic.com",
            route_id: "anthropic",
            model: None,
        },
        SetupProvider::Openai => ProviderDefaults {
            name: "openai",
            base_url: "https://api.openai.com",
            route_id: "openai",
            model: None,
        },
    }
}

pub(in crate::commands) fn run(args: &SetupArgs) -> Outcome {
    finish(COMMAND, execute(args), |summary| summary)
}

/// `init`'s failures carry `init`'s codes; here the same message is reported under the gateway
/// family, by what it is about: the key file and the keyring are credential-mechanism failures,
/// everything else (the project path, the ignore file) is invalid input.
fn from_init(failure: init::Failure) -> Failure {
    match failure.pointer {
        "/key" | "/keyring" => credential_error(&failure.message, failure.pointer),
        pointer => invalid(&failure.message, pointer),
    }
}

/// How the manifest file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManifestState {
    Created,
    Merged,
    Replaced,
}

impl ManifestState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Merged => "merged",
            Self::Replaced => "replaced",
        }
    }
}

struct Route {
    id: String,
    provider: &'static str,
    base_url: String,
    model: String,
    credential_ref: String,
}

fn execute(args: &SetupArgs) -> Result<Value, Failure> {
    // #1141 review (MEDIUM): `--key-id` is interpolated UNQUOTED into the copy-paste commands
    // this command prints, so an id carrying a shell metacharacter re-opens #1070 on this
    // surface. `init` validates its own; this is the same validator, applied before any file is
    // touched or any key is read.
    validate_key_id(&args.key_id).map_err(from_init)?;
    let provider = defaults(args.provider);
    let route = Route {
        id: args
            .route_id
            .clone()
            .unwrap_or_else(|| provider.route_id.to_owned()),
        provider: provider.name,
        base_url: args
            .base_url
            .clone()
            .unwrap_or_else(|| provider.base_url.to_owned()),
        model: match (&args.model, provider.model) {
            (Some(model), _) => model.clone(),
            (None, Some(model)) => model.to_owned(),
            (None, None) => {
                return Err(invalid(
                    "--model is required for this provider; setup does not guess a model name",
                    "/model",
                ));
            }
        },
        credential_ref: format!("secret_{}", provider.name),
    };

    let project = init::resolve_project(args.project.as_deref()).map_err(from_init)?;
    let root = project.join(RUNTIME_DIRECTORY);
    init::refuse_symlink(&root, "/root").map_err(from_init)?;
    std::fs::create_dir_all(&root)
        .map_err(|_| invalid("the .graphhelm directory could not be created", "/root"))?;

    // The manifest is prepared and validated first, and a same-id route is refused here: nothing
    // below has happened yet, so a refusal leaves the project exactly as it was, and the operator
    // is not asked for a key the command is about to discard.
    let manifest_path = root.join(MANIFEST_FILE);
    init::refuse_symlink(&manifest_path, "/manifest").map_err(from_init)?;
    let _ = merged_manifest(&manifest_path, &route, args.replace)?;

    let sealing = init::ensure_sealing_keyring(&root, &args.key_id).map_err(from_init)?;
    let keyring = root.join(KEYRING_DIRECTORY);
    let broker_dir = root.join(BROKER_DIRECTORY);

    // The key: read once, stored, and out of scope before anything is printed.
    let value = read_key_from_process_stdin(provider.name)?;
    let probe_passphrase = sealing
        .passphrase
        .expose(|bytes| SecretBytes::new(bytes.to_vec()));
    // The key read can wait on a person, so it happens before the bounded manifest lock. Once the
    // key is present, re-read and compose under the same cross-process lock `route set` uses.
    let _manifest_lock = route::acquire_manifest_lock(&manifest_path)?;
    let (manifest_text, manifest_state) = merged_manifest(&manifest_path, &route, args.replace)?;
    store_credential(
        &broker_dir,
        &keyring,
        &args.key_id,
        sealing.passphrase,
        SecretReference {
            id: route.credential_ref.clone(),
            provider: route.provider.to_owned(),
            usable_by: vec![route.id.clone()],
        },
        value,
    )?;

    route::write_atomically(&manifest_path, &manifest_text)?;

    let gitignore_state =
        init::ensure_gitignore(&project, &project.join(GITIGNORE_FILE)).map_err(from_init)?;

    let manifest = RouteManifest::from_json(&manifest_text)
        .map_err(|error| invalid(&error.to_string(), "/manifest"))?;
    let probe_result = probe::probe_loaded(
        &manifest,
        &route.id,
        Some(&broker_dir),
        Some(&keyring),
        Some(&args.key_id),
        || Ok(probe_passphrase),
    )?;

    Ok(json!({
        "project": args
            .project
            .as_deref()
            .map_or_else(|| ".".to_owned(), |given| given.to_string_lossy().into_owned()),
        "manifest": {
            "path": format!("{RUNTIME_DIRECTORY}/{MANIFEST_FILE}"),
            "state": manifest_state.as_str(),
        },
        "route": {
            "id": route.id,
            "provider": route.provider,
            "model": route.model,
            "baseUrl": route.base_url,
            "credentialRef": route.credential_ref,
        },
        "provider": route.provider,
        "credentialRef": route.credential_ref,
        "credential": {
            "broker": format!("{RUNTIME_DIRECTORY}/{BROKER_DIRECTORY}"),
            "usableBy": [route.id],
        },
        "key": {
            "path": format!("{RUNTIME_DIRECTORY}/{KEY_FILE}"),
            "state": sealing.key_state.as_str(),
        },
        "keyring": {
            "path": format!("{RUNTIME_DIRECTORY}/{KEYRING_DIRECTORY}"),
            "keyId": args.key_id,
            "state": sealing.keyring_state.as_str(),
        },
        "gitignore": { "path": GITIGNORE_FILE, "state": gitignore_state.as_str() },
        "probe": probe::render(probe_result),
        "next": next_steps(&NextPaths {
            provider: args.provider,
            route_id: &route.id,
            manifest: &manifest_path,
            broker: &broker_dir,
            keyring: &keyring,
            key: &root.join(KEY_FILE),
            key_id: &args.key_id,
        }),
    }))
}

/// The manifest document with `route` in it, serialized, and already validated as a whole
/// through [`RouteManifest::from_json`] — the same function every reader of the file uses, so a
/// document this returns is one `routes`, `probe`, `serve` and `graph synthesize` will accept.
/// Nothing is written here.
fn merged_manifest(
    path: &Path,
    route: &Route,
    replace: bool,
) -> Result<(String, ManifestState), Failure> {
    let (mut document, existed) = load_document(path)?;
    let routes = match document
        .entry("routes")
        .or_insert_with(|| Value::Array(Vec::new()))
    {
        Value::Array(routes) => routes,
        _ => return Err(unusable_manifest()),
    };
    let entry = json!({
        "id": route.id,
        "provider": route.provider,
        "transport": "direct_api",
        "authentication": "api_key",
        "billingMode": "per_token",
        "baseUrl": route.base_url,
        "model": route.model,
        "credentialRef": route.credential_ref,
        "profiles": [PROFILE],
        "enabled": true,
    });
    let position = routes
        .iter()
        .position(|existing| existing.get("id").and_then(Value::as_str) == Some(route.id.as_str()));
    let state = match (position, replace) {
        (Some(_), false) => {
            return Err(invalid(
                "the manifest already declares a route with this id; pass --replace to swap it, or --route-id for a different one",
                "/route",
            ));
        }
        (Some(index), true) => {
            routes[index] = entry;
            ManifestState::Replaced
        }
        (None, _) => {
            routes.push(entry);
            if existed {
                ManifestState::Merged
            } else {
                ManifestState::Created
            }
        }
    };
    document
        .entry("manifestVersion")
        .or_insert_with(|| json!(MANIFEST_VERSION));

    let mut text = serde_json::to_string_pretty(&Value::Object(document))
        .map_err(|_| invalid("the manifest could not be serialized", "/manifest"))?;
    text.push('\n');
    RouteManifest::from_json(&text).map_err(|error| invalid(&error.to_string(), "/manifest"))?;
    Ok((text, state))
}

fn unusable_manifest() -> Failure {
    invalid(
        "manifest.json exists and is not a JSON object with an optional \"routes\" array; fix or move it aside",
        "/manifest",
    )
}

/// The existing document as a JSON object (an absent file is an empty one), read through the
/// same bounded reader every gateway command uses. A UTF-8 BOM is stripped rather than refused,
/// as `init` does for `.mcp.json`. Returns whether the file existed, for the reported state.
fn load_document(path: &Path) -> Result<(Map<String, Value>, bool), Failure> {
    if !path.exists() {
        return Ok((Map::new(), false));
    }
    let bytes = read_bounded_manifest(path).map_err(|error| match error {
        ManifestReadError::Unreadable => invalid("manifest.json could not be read", "/manifest"),
        ManifestReadError::TooLarge => invalid(
            "manifest.json exceeds the maximum supported size",
            "/manifest",
        ),
    })?;
    let text = String::from_utf8(bytes).map_err(|_| unusable_manifest())?;
    let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|_| unusable_manifest())?;
    match value {
        Value::Object(map) => Ok((map, true)),
        _ => Err(unusable_manifest()),
    }
}

/// The broker path `gateway credential set` takes (`credential.rs::execute_set`), with the
/// passphrase from `serve.key` instead of `GRAPHHELM_GATEWAY_KEY`: `open_or_create` on the
/// project's broker directory, then `store`. Storing over an existing reference is a rotation —
/// the operator pasted a new key on purpose.
fn store_credential(
    broker_dir: &Path,
    keyring_dir: &Path,
    key_id: &str,
    passphrase: SecretBytes,
    reference: SecretReference,
    value: SecretBytes,
) -> Result<(), Failure> {
    let broker_dir = broker_dir.to_path_buf();
    let keyring_dir = keyring_dir.to_path_buf();
    let key_id = key_id.to_owned();
    runtime()?.block_on(async move {
        let mut broker =
            CredentialBroker::open_or_create(&broker_dir, &keyring_dir, &key_id, passphrase)
                .await
                .map_err(|error| broker_failure(&error))?;
        broker
            .store(reference, value)
            .await
            .map_err(|error| broker_failure(&error))
    })
}

/// The process's own stdin and stderr, decided by whether stdin is a terminal. Everything
/// testable lives in [`read_key`]; this function only chooses the plumbing.
fn read_key_from_process_stdin(provider: &str) -> Result<SecretBytes, Failure> {
    let stdin = std::io::stdin();
    let interactive = stdin.is_terminal();
    let mut stderr = std::io::stderr().lock();
    if interactive {
        let (result, hidden) =
            with_echo_off(|| read_key(&mut stdin.lock(), &mut stderr, provider, true));
        // The terminal did not echo the newline the operator typed, so the next line of output
        // would otherwise start on the prompt's line.
        let _ = stderr.write_all(b"\n");
        if !hidden {
            let _ = stderr.write_all(
                b"note: the terminal did not accept echo off; the key may have been visible while typed. Prefer the piped form: echo $KEY | graphhelm gateway setup ...\n",
            );
        }
        result
    } else {
        read_key(&mut stdin.lock(), &mut stderr, provider, false)
    }
}

/// Reads the key: one line from `input`, its line ending and surrounding whitespace trimmed,
/// refused when empty. When `interactive`, the prompt is written to `prompt` first (the caller
/// has turned echo off around this call). The bytes live in a `Zeroizing` buffer on every path,
/// including the refusals, exactly as `credential set`'s reader does.
fn read_key(
    input: &mut impl BufRead,
    prompt: &mut impl Write,
    provider: &str,
    interactive: bool,
) -> Result<SecretBytes, Failure> {
    if interactive {
        let _ = write!(prompt, "Paste the {provider} API key (input hidden): ");
        let _ = prompt.flush();
    }
    let mut buffer = zeroize::Zeroizing::new(Vec::<u8>::new());
    let read = input
        .read_until(b'\n', &mut buffer)
        .map_err(|_| invalid("the API key could not be read from stdin", "/stdin"))?;
    if read == 0 {
        return Err(invalid(
            "an API key is required: paste it at the prompt, or pipe it on stdin",
            "/stdin",
        ));
    }
    while buffer.last().is_some_and(u8::is_ascii_whitespace) {
        buffer.pop();
    }
    let leading = buffer
        .iter()
        .take_while(|byte| byte.is_ascii_whitespace())
        .count();
    if leading > 0 {
        buffer.drain(..leading);
    }
    if buffer.is_empty() {
        return Err(invalid("the API key must not be empty", "/stdin"));
    }
    if std::str::from_utf8(&buffer).is_err() {
        return Err(credential_error("the API key is not valid UTF-8", "/stdin"));
    }
    Ok(SecretBytes::new(std::mem::take(&mut *buffer)))
}

/// Runs `read` with the terminal's echo off, restoring it afterwards, and reports whether echo
/// was actually off. A terminal that refuses the mode change still gets the read (the operator
/// is warned that the key may have been visible) rather than a refusal with no way forward.
#[cfg(unix)]
fn with_echo_off<T>(read: impl FnOnce() -> T) -> (T, bool) {
    use std::os::fd::AsRawFd;
    let fd = std::io::stdin().as_raw_fd();
    let mut original = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: `tcgetattr` writes one `termios` into a buffer of exactly that type; the buffer is
    // read back only after the call reported success.
    if unsafe { libc::tcgetattr(fd, original.as_mut_ptr()) } != 0 {
        return (read(), false);
    }
    // SAFETY: initialized by the successful `tcgetattr` above.
    let original = unsafe { original.assume_init() };
    let mut hidden = original;
    hidden.c_lflag &= !libc::ECHO;
    // SAFETY: `hidden` is a fully initialized `termios` that outlives the call.
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &hidden) } != 0 {
        return (read(), false);
    }
    // RESTORE ON EVERY EXIT, not only the happy one (#1141 review, LOW): a panic inside `read`
    // would otherwise leave the operator's terminal with echo off after the process dies.
    struct Restore(i32, libc::termios);
    impl Drop for Restore {
        fn drop(&mut self) {
            // SAFETY: `self.1` is the fully initialized `termios` read from this descriptor.
            unsafe { libc::tcsetattr(self.0, libc::TCSANOW, &self.1) };
        }
    }
    let restore = Restore(fd, original);
    let result = read();
    drop(restore);
    (result, true)
}

#[cfg(windows)]
fn with_echo_off<T>(read: impl FnOnce() -> T) -> (T, bool) {
    use windows_sys::Win32::System::Console::{
        ENABLE_ECHO_INPUT, GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE, SetConsoleMode,
    };
    // SAFETY: `GetStdHandle` takes a constant and returns a handle the process does not own and
    // must not close; `GetConsoleMode` writes one `u32` through the pointer given, which points
    // at a local that outlives the call.
    let (handle, mode) = unsafe {
        let handle = GetStdHandle(STD_INPUT_HANDLE);
        let mut mode = 0_u32;
        if GetConsoleMode(handle, &raw mut mode) == 0 {
            return (read(), false);
        }
        (handle, mode)
    };
    // SAFETY: `handle` was just returned for the process's own stdin and `mode` was read from it.
    if unsafe { SetConsoleMode(handle, mode & !ENABLE_ECHO_INPUT) } == 0 {
        return (read(), false);
    }
    // RESTORE ON EVERY EXIT (#1141 review, LOW): see the Unix arm.
    struct Restore(isize, u32);
    impl Drop for Restore {
        fn drop(&mut self) {
            // SAFETY: the handle is the process's own stdin and the mode was read from it.
            unsafe { SetConsoleMode(self.0 as _, self.1) };
        }
    }
    let restore = Restore(handle as isize, mode);
    let result = read();
    drop(restore);
    (result, true)
}

#[cfg(not(any(unix, windows)))]
fn with_echo_off<T>(read: impl FnOnce() -> T) -> (T, bool) {
    (read(), false)
}

struct NextPaths<'a> {
    provider: SetupProvider,
    route_id: &'a str,
    manifest: &'a Path,
    broker: &'a Path,
    keyring: &'a Path,
    key: &'a Path,
    key_id: &'a str,
}

/// The commands that come next, with this run's paths filled in, quoted the way `init` quotes
/// its own (`quoted_bash`/`quoted_powershell`: the one place absolute paths appear, because the
/// operator copies these into a shell from anywhere). The route is NAMED on each of them — setup
/// wires a route and never selects one. The key is never in these strings: the commands read the
/// broker's passphrase from `serve.key`, whose value is not printed either.
fn next_steps(paths: &NextPaths<'_>) -> Vec<Value> {
    let (manifest, broker, keyring, key) = (
        quoted_bash(paths.manifest),
        quoted_bash(paths.broker),
        quoted_bash(paths.keyring),
        quoted_bash(paths.key),
    );
    let (ps_manifest, ps_broker, ps_keyring, ps_key) = (
        quoted_powershell(paths.manifest),
        quoted_powershell(paths.broker),
        quoted_powershell(paths.keyring),
        quoted_powershell(paths.key),
    );
    let key_id = paths.key_id;
    let route_id = paths.route_id;
    let route_flag = match paths.provider {
        SetupProvider::Typesafe => format!("--route <draft route> --judge-route {route_id}"),
        SetupProvider::Anthropic | SetupProvider::Openai => format!("--route {route_id}"),
    };
    let out_path: PathBuf = PathBuf::from("g1.json");
    let (out, ps_out) = (quoted_bash(&out_path), quoted_powershell(&out_path));
    vec![
        json!({
            "step": "export the broker passphrase from serve.key (the same bytes setup stored the key under; never passed as a flag)",
            "powershell": format!("$env:GRAPHHELM_GATEWAY_KEY = (Get-Content -Raw {ps_key}).Trim()"),
            "bash": format!("export GRAPHHELM_GATEWAY_KEY=\"$(cat {key})\""),
        }),
        json!({
            "step": "probe the route again whenever you want (quota-free: it proves the credential leases, it places no model call)",
            "powershell": format!("graphhelm gateway probe --manifest {ps_manifest} --route {route_id} --broker {ps_broker} --keyring {ps_keyring} --key-id {key_id}"),
            "bash": format!("graphhelm gateway probe --manifest {manifest} --route {route_id} --broker {broker} --keyring {keyring} --key-id {key_id}"),
        }),
        json!({
            "step": "compile a goal through the route (the route is named on every call; nothing selects it for you)",
            "powershell": format!("graphhelm graph synthesize --goal '<goal>' --allow-program cargo --manifest {ps_manifest} {route_flag} --broker {ps_broker} --keyring {ps_keyring} --key-id {key_id} --out {ps_out}"),
            "bash": format!("graphhelm graph synthesize --goal '<goal>' --allow-program cargo --manifest {manifest} {route_flag} --broker {broker} --keyring {keyring} --key-id {key_id} --out {out}"),
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const SENTINEL: &str = "sk-SENTINEL-unit-0123456789";

    fn read(input: &[u8], interactive: bool) -> (Result<Vec<u8>, Failure>, String) {
        let mut prompt = Vec::new();
        let mut input = input;
        let result = read_key(&mut input, &mut prompt, "typesafe", interactive)
            .map(|secret| secret.expose(|bytes| bytes.to_vec()));
        (result, String::from_utf8(prompt).unwrap())
    }

    #[test]
    fn the_terminal_path_prompts_and_reads_one_trimmed_line() {
        let (result, prompt) = read(format!("  {SENTINEL}\r\nsecond line\n").as_bytes(), true);
        assert_eq!(result.ok().expect("accepted"), SENTINEL.as_bytes());
        assert_eq!(prompt, "Paste the typesafe API key (input hidden): ");
    }

    #[test]
    fn the_piped_path_reads_the_same_line_and_prints_no_prompt() {
        let (result, prompt) = read(format!("{SENTINEL}\n").as_bytes(), false);
        assert_eq!(result.ok().expect("accepted"), SENTINEL.as_bytes());
        assert!(prompt.is_empty(), "{prompt:?}");
        // No line ending at all is still one line.
        let (result, _) = read(SENTINEL.as_bytes(), false);
        assert_eq!(result.ok().expect("accepted"), SENTINEL.as_bytes());
    }

    #[test]
    fn an_empty_key_is_refused_on_both_paths() {
        // A line of only spaces is built, not spelled: the source invariant refuses a literal
        // run of whitespace, since operators read those verbatim.
        let blank = format!("{}\r\n", " ".repeat(3));
        let cases = [
            ("", true),
            ("", false),
            ("\n", true),
            (blank.as_str(), false),
        ];
        for (input, interactive) in cases {
            let (result, _) = read(input.as_bytes(), interactive);
            let Err(failure) = result else {
                panic!("{input:?} must be refused");
            };
            assert_eq!(failure.code, super::super::INVALID_CODE, "{input:?}");
            assert_eq!(failure.pointer, "/stdin");
        }
    }

    #[test]
    fn invalid_utf8_is_refused_naming_the_encoding_and_nothing_else() {
        let (result, prompt) = read(&[0xFF_u8, 0xFE, b'\n'], true);
        let failure = result.err().unwrap();
        assert_eq!(failure.code, super::super::CREDENTIAL_CODE);
        assert!(
            failure.message.to_lowercase().contains("utf-8"),
            "{}",
            failure.message
        );
        assert!(!failure.message.contains('\u{fffd}'));
        assert_eq!(prompt, "Paste the typesafe API key (input hidden): ");
    }

    #[test]
    fn only_typesafe_has_a_default_model() {
        assert_eq!(defaults(SetupProvider::Typesafe).model, Some("jev-latest"));
        assert_eq!(defaults(SetupProvider::Anthropic).model, None);
        assert_eq!(defaults(SetupProvider::Openai).model, None);
        for provider in [
            SetupProvider::Typesafe,
            SetupProvider::Anthropic,
            SetupProvider::Openai,
        ] {
            assert!(defaults(provider).base_url.starts_with("https://"));
        }
    }

    #[test]
    fn a_manifest_that_is_not_an_object_is_refused_before_anything_is_written() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("manifest.json");
        std::fs::write(&path, "[]").unwrap();
        let route = Route {
            id: "judge".to_owned(),
            provider: "typesafe",
            base_url: "https://api.typesafe.ai".to_owned(),
            model: "jev-latest".to_owned(),
            credential_ref: "secret_typesafe".to_owned(),
        };
        let failure = merged_manifest(&path, &route, false).err().unwrap();
        assert_eq!(failure.pointer, "/manifest");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[]");
    }
}
