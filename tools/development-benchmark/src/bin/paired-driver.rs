//! The paired driver: builds both arms' contexts and writes the run directory the runner reads.
//!
//! `--provider fake` is the only provider this binary knows so far, and that is the dry-run
//! contract: the fake reports `context bytes / 4` as its input count, labelled `fake-provider`,
//! and the arms declare `modelRoute: fake` -- two labels between a dry-run number and a live one.
//! The live provider arrives as a second arm of the same match, not a fork of this binary.
//!
//! Refusal discipline is inherited, not reinvented: the manifest loads through `load_manifest`
//! (digest-frozen), the frozen files verify through `verify_frozen_files`, every retrieval
//! artifact passes the SHIPPED plan discipline inside `capsule_for_case` (stale refuses, escapes
//! refuse), and the oracle is read through `oracle_evidence_paths`, which cannot see the answer.
//!
//! Exit codes: 0 a run directory was produced; 2 a typed refusal; 64 usage error.

use graphhelm_development_benchmark::{
    BenchmarkRefusal, Manifest, RetrievalArtifact, capsule_for_case, live_arm_cost, load_manifest,
    naive_context, oracle_evidence_paths, transmitted_settings, verify_frozen_files,
};

struct Arguments {
    manifest: String,
    retrieval: String,
    repo_root: String,
    out: String,
    provider: String,
    clock: String,
    binary_digest: String,
    environment: String,
    /// Live-provider coordinates: gateway route manifest, route id, broker/keyring dirs, key id.
    /// All five required together when `--provider live`; the passphrase rides
    /// `GRAPHHELM_GATEWAY_KEY` exactly as `graphhelm gateway probe` reads it, and this binary
    /// never prints or stores any of it.
    gateway_manifest: Option<String>,
    route: Option<String>,
    broker: Option<String>,
    keyring: Option<String>,
    key_id: Option<String>,
}

/// What one arm's cost measurement runs through: the fake rule, or a live adapter.
enum Provider<'route> {
    Fake,
    Live {
        adapter: graphhelm_model_gateway::byok::ByokAdapter<'route>,
        key: graphhelm_events::SecretBytes,
        route_id: String,
        /// The provider KIND ("anthropic" | "openai"), read from the route: the settings label
        /// derives from it, because what a call transmits differs per provider.
        provider: String,
    },
}

/// Which provider the settings label is derived for.
fn provider_kind<'a>(provider: &'a Provider<'_>) -> &'a str {
    match provider {
        Provider::Fake => "fake",
        Provider::Live { provider, .. } => provider.as_str(),
    }
}

fn main() {
    std::process::exit(run());
}

fn usage() -> i32 {
    eprintln!(
        "usage: paired-driver --manifest <path> --retrieval <dir> --repo-root <dir> --out <dir> \
         --provider fake --clock <instant> --binary-digest <id> --environment <record>"
    );
    64
}

fn parse() -> Result<Arguments, i32> {
    let mut manifest = None;
    let mut retrieval = None;
    let mut repo_root = None;
    let mut out = None;
    let mut provider = None;
    let mut clock = None;
    let mut binary_digest = None;
    let mut environment = None;
    let mut gateway_manifest = None;
    let mut route = None;
    let mut broker = None;
    let mut keyring = None;
    let mut key_id = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        let slot = match argument.as_str() {
            "--manifest" => &mut manifest,
            "--retrieval" => &mut retrieval,
            "--repo-root" => &mut repo_root,
            "--out" => &mut out,
            "--provider" => &mut provider,
            "--clock" => &mut clock,
            "--binary-digest" => &mut binary_digest,
            "--environment" => &mut environment,
            "--gateway-manifest" => &mut gateway_manifest,
            "--route" => &mut route,
            "--broker" => &mut broker,
            "--keyring" => &mut keyring,
            "--key-id" => &mut key_id,
            other => {
                eprintln!("unknown argument `{other}`");
                return Err(usage());
            }
        };
        *slot = arguments.next();
    }
    match (
        manifest,
        retrieval,
        repo_root,
        out,
        provider,
        clock,
        binary_digest,
        environment,
    ) {
        (
            Some(manifest),
            Some(retrieval),
            Some(repo_root),
            Some(out),
            Some(provider),
            Some(clock),
            Some(binary_digest),
            Some(environment),
        ) => Ok(Arguments {
            manifest,
            retrieval,
            repo_root,
            out,
            provider,
            clock,
            binary_digest,
            environment,
            gateway_manifest,
            route,
            broker,
            keyring,
            key_id,
        }),
        _ => Err(usage()),
    }
}

fn run() -> i32 {
    let arguments = match parse() {
        Ok(arguments) => arguments,
        Err(code) => return code,
    };
    match arguments.provider.as_str() {
        "fake" => match drive(&arguments, &Provider::Fake) {
            Ok(()) => 0,
            Err(refusal) => {
                println!("{refusal:?}");
                2
            }
        },
        "live" => run_live(&arguments),
        other => {
            eprintln!("provider `{other}` is unknown; `fake` or `live`");
            64
        }
    }
}

/// The live wiring, kept to coordinates-and-lease: everything measured happens in the same
/// `drive` the dry run exercises, with a `Provider::Live` in place of the fake rule. Neither the
/// passphrase nor the leased key is ever printed or written.
fn run_live(arguments: &Arguments) -> i32 {
    let (Some(gateway_manifest), Some(route_id), Some(broker), Some(keyring), Some(key_id)) = (
        arguments.gateway_manifest.as_ref(),
        arguments.route.as_ref(),
        arguments.broker.as_ref(),
        arguments.keyring.as_ref(),
        arguments.key_id.as_ref(),
    ) else {
        eprintln!(
            "--provider live requires --gateway-manifest --route --broker --keyring --key-id"
        );
        return 64;
    };
    // The 256 KiB bound holds AT the boundary: refused by name before the allocation, so an
    // oversized operator file is a named refusal instead of an allocator event (K on #531).
    const MANIFEST_BOUND: u64 = 256 * 1024;
    match std::fs::metadata(gateway_manifest) {
        Ok(metadata) if metadata.len() > MANIFEST_BOUND => {
            eprintln!(
                "{gateway_manifest}: {} bytes exceeds the {MANIFEST_BOUND}-byte route-manifest \
                 bound; refused before reading",
                metadata.len()
            );
            return 2;
        }
        Err(error) => {
            eprintln!("{gateway_manifest}: {error}");
            return 2;
        }
        Ok(_) => {}
    }
    let manifest_text = match std::fs::read_to_string(gateway_manifest) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("{gateway_manifest}: {error}");
            return 2;
        }
    };
    let manifest = match graphhelm_gateway::manifest::RouteManifest::from_json(&manifest_text) {
        Ok(manifest) => manifest,
        Err(error) => {
            eprintln!("{gateway_manifest}: {error}");
            return 2;
        }
    };
    let Some(route) = manifest
        .routes()
        .iter()
        .find(|route| route.id() == route_id.as_str())
    else {
        eprintln!("route `{route_id}` is not in {gateway_manifest}");
        return 2;
    };

    // The operator's kill switch, honoured BEFORE the key is read and BEFORE the broker opens:
    // this driver is the manifest consumer that spends money, and it was the only one ignoring
    // `enabled` (K's hold on #531). Probe refuses it, eligibility filters it, so does this.
    if !route.enabled() {
        eprintln!("route `{route_id}` is disabled; the kill switch stops this spend");
        return 2;
    }
    let passphrase = match read_gateway_key() {
        Ok(passphrase) => passphrase,
        Err(message) => {
            eprintln!("{message}");
            return 2;
        }
    };
    let Some(credential_ref) = route.credential_ref().map(str::to_owned) else {
        eprintln!("route `{route_id}` carries no credentialRef");
        return 2;
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("tokio runtime: {error}");
            return 2;
        }
    };
    let broker_dir = std::path::PathBuf::from(broker);
    let keyring_dir = std::path::PathBuf::from(keyring);
    let lease = runtime.block_on(async {
        let opened = graphhelm_model_gateway::broker::CredentialBroker::open(
            &broker_dir,
            &keyring_dir,
            key_id,
            passphrase,
        )
        .await
        .map_err(|error| format!("broker: {error}"))?;
        opened
            .lease(&credential_ref, route_id)
            .await
            .map_err(|error| format!("lease: {error}"))
    });
    let key = match lease {
        Ok(key) => key,
        Err(message) => {
            eprintln!("{message}");
            return 2;
        }
    };
    let transport: std::sync::Arc<dyn graphhelm_model_gateway::transport::HttpTransport> =
        std::sync::Arc::new(graphhelm_model_gateway::transport::UreqTransport::new());
    let provider = Provider::Live {
        adapter: graphhelm_model_gateway::byok::ByokAdapter::new(route, transport),
        key,
        route_id: route_id.clone(),
        provider: route.provider().to_owned(),
    };
    match drive(arguments, &provider) {
        Ok(()) => 0,
        Err(refusal) => {
            println!("{refusal:?}");
            2
        }
    }
}

/// GRAPHHELM_GATEWAY_KEY: 64 lowercase hex characters decoded to 32 bytes -- the same contract
/// `graphhelm gateway probe` enforces. The encoded form is zeroized after decoding.
fn read_gateway_key() -> Result<graphhelm_events::SecretBytes, String> {
    const MALFORMED: &str = "GRAPHHELM_GATEWAY_KEY must supply 64 lowercase hex characters";
    let encoded = std::env::var("GRAPHHELM_GATEWAY_KEY").map_err(|_| MALFORMED.to_owned())?;
    let encoded = zeroize::Zeroizing::new(encoded.into_bytes());
    if encoded.len() != 64 {
        return Err(MALFORMED.to_owned());
    }
    let value = |byte: u8| match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(MALFORMED.to_owned()),
    };
    let mut bytes = zeroize::Zeroizing::new(Vec::<u8>::with_capacity(32));
    for pair in encoded.chunks_exact(2) {
        bytes.push((value(pair[0])? << 4) | value(pair[1])?);
    }
    Ok(graphhelm_events::SecretBytes::new(std::mem::take(
        &mut *bytes,
    )))
}

/// The fake provider's whole contract: input tokens = ceil(bytes / 4), labelled as fake.
fn fake_input_tokens(context: &[u8]) -> u64 {
    (context.len() as u64).div_ceil(4)
}

fn drive(arguments: &Arguments, provider: &Provider) -> Result<(), BenchmarkRefusal> {
    let text = std::fs::read_to_string(&arguments.manifest).map_err(|error| {
        BenchmarkRefusal::Unreadable {
            detail: format!("{}: {error}", arguments.manifest),
        }
    })?;
    let manifest: Manifest = load_manifest(&text)?;
    let bench_root = std::path::Path::new(&arguments.manifest)
        .parent()
        .map_or_else(
            || std::path::PathBuf::from("."),
            std::path::Path::to_path_buf,
        );
    verify_frozen_files(&manifest, &bench_root)?;

    let repo_root = std::path::Path::new(&arguments.repo_root);
    // The artifacts' coordinates were produced against ONE source tree; `capsule_for_case` slices
    // whatever tree `--repo-root` points at. Nothing bound the two, so line ranges computed over
    // snapshot A were being applied to checkout B -- and the store check added earlier does not
    // help: it binds the STORE to a declared revision, never either identifier to the tree the
    // driver actually reads (Codex P1).
    //
    // Derived, not declared: the tree hash comes from the repository itself, so the operator
    // cannot assert agreement into existence. The artifacts carry a git tree id (that is what the
    // generator was given), so `HEAD^{tree}` is the identity to compare.
    let observed_tree = std::process::Command::new("git")
        .args(["rev-parse", "HEAD^{tree}"])
        .current_dir(repo_root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .ok_or_else(|| BenchmarkRefusal::Unreadable {
            detail: format!(
                "the repo root {} is not a git checkout, so its identity cannot be bound to the \
                 artifacts' coordinates",
                repo_root.display()
            ),
        })?;
    // `HEAD^{tree}` names the COMMITTED tree, and the readers below read the WORKING one. A
    // modified tracked file — or an untracked file at a path an artifact names — leaves the check
    // passing while `naive_context` and `capsule_for_case` slice different bytes (Codex P1). The
    // tree id cannot see that by construction, so the working tree has to be clean for it to mean
    // anything.
    let dirty = std::process::Command::new("git")
        // `--ignored` as well: an IGNORED file at a path an artifact names is read by the arms
        // exactly like any other, and `--untracked-files=all` does not list it (Codex P1, after
        // my clean-tree fix). The question is "does the working tree hold bytes the committed
        // tree does not", and ignoring is not an answer to it.
        .args([
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--ignored=matching",
        ])
        .current_dir(repo_root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .ok_or_else(|| BenchmarkRefusal::Unreadable {
            detail: "the repo root's working-tree state could not be read".to_owned(),
        })?;
    // NARROWED, because my own previous fix broke ordinary use: adding `--ignored` made `target/`
    // — which Cargo creates under any repo root the driver is built in — a blocking dirty entry,
    // so no normal drive could run (Codex P1 on the fix itself).
    //
    // The question was never "is the tree pristine". It is "do the bytes the arms will READ differ
    // from the tree the artifacts declare", so only entries at paths the corpus actually names
    // matter. An ignored `target/` is invisible to the arms; an ignored file at an oracle's
    // required path is not.
    let dirty_paths: Vec<&str> = dirty
        .lines()
        .filter_map(|line| line.get(3..))
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .collect();
    let read_paths: std::collections::BTreeSet<String> = manifest
        .cases
        .iter()
        .flat_map(|case| {
            // BOTH sides of what the arms read, not just the oracle's (Codex). The naive arm
            // reads the oracle's required evidence; the COMPILED arm reads the paths its
            // retrieval artifact names, and those are frequently different files. Populating this
            // set from the oracle alone left every compiled-arm path outside the check — the
            // question is "do the bytes the arms READ differ", and half the reading was missing
            // from the population.
            let oracle = bench_root.join("oracle").join(format!("{}.json", case.id));
            let mut paths = oracle_evidence_paths(&oracle)
                .map(|(evidence, _)| evidence)
                .unwrap_or_default();
            let artifact =
                std::path::Path::new(&arguments.retrieval).join(format!("{}.json", case.id));
            if let Ok(text) = std::fs::read_to_string(&artifact)
                && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
                && let Some(hits) = value["hits"].as_array()
            {
                paths.extend(
                    hits.iter()
                        .filter_map(|hit| hit["path"].as_str())
                        .map(str::to_owned),
                );
            }
            paths
        })
        .collect();
    let colliding: Vec<&&str> = dirty_paths
        .iter()
        // BOTH directions, because git reports an ignored DIRECTORY as a single entry
        // (`some/dir/`) rather than listing the files inside it: corpus evidence living in an
        // ignored directory slips past a one-way prefix test. That is the hole on the other side
        // of my own `--ignored` fix (Codex P1) — I closed the case where an ignored FILE sits at
        // a read path and left open the case where an ignored DIRECTORY contains one.
        .filter(|path| {
            read_paths.iter().any(|read| {
                // Through the crate's canonical form, the same one recall and the oracle check
                // use: comparing raw strings made  and  different paths
                // (Codex P1). Third finding in this file about comparing paths as if a path had
                // one spelling; this is the shared answer rather than a third local patch.
                let path = graphhelm_development_benchmark::canonical_path(path);
                let read = graphhelm_development_benchmark::canonical_path(read);
                path.starts_with(&read) || read.starts_with(&path)
            })
        })
        .collect();
    if !colliding.is_empty() {
        return Err(BenchmarkRefusal::Unreadable {
            detail: format!(
                "the repo root holds uncommitted bytes at {} path(s) the corpus reads (first: {}); \
                 the tree id names the COMMITTED bytes while the arms would read these",
                colliding.len(),
                colliding[0]
            ),
        });
    }
    let retrieval_root = std::path::Path::new(&arguments.retrieval);
    // The freeze verified `<manifest parent>/retrieval`; this argument decides what is actually
    // READ. Two paths that must agree are a state that can disagree, and it did: an approved
    // frozen directory passed verification while byte-different artifacts from another directory
    // determined the compiled contexts (Codex P1). The fourth digest verified something nobody
    // consumed.
    //
    // Refused rather than re-verified, and refused rather than silently overridden: the operator
    // who pointed elsewhere is told, instead of watching their argument be ignored. Compared
    // after canonicalisation so a different spelling of the same directory is not a divergence.
    let verified_retrieval = bench_root.join("retrieval");
    let same_directory = match (
        retrieval_root.canonicalize(),
        verified_retrieval.canonicalize(),
    ) {
        (Ok(given), Ok(verified)) => given == verified,
        // An unreadable path is not an agreement. The read below would fail anyway; refusing here
        // names the reason.
        _ => false,
    };
    if !same_directory {
        return Err(BenchmarkRefusal::Unreadable {
            detail: format!(
                "the retrieval directory given ({}) is not the one the freeze verified ({}): the \
                 artifacts that would be read are not the artifacts that were checked",
                retrieval_root.display(),
                verified_retrieval.display()
            ),
        });
    }
    let out_root = std::path::Path::new(&arguments.out);

    // COMPLETED WORK PERSISTS AS IT COMPLETES (Codex #531 P1): a live drive that fails part way
    // must not discard the paid calls that already happened -- each case's receipt is written the
    // moment its pair of calls finishes. The run stays INELIGIBLE for comparison until the very
    // end, because `arms.json` is written last and the reader requires it: eligibility is marked
    // by the one file whose absence the reader already refuses, not by a cleanup step.
    std::fs::create_dir_all(out_root.join("cases")).map_err(|error| {
        BenchmarkRefusal::Unreadable {
            detail: format!("{}: {error}", out_root.display()),
        }
    })?;
    let write = |path: &std::path::Path, value: &serde_json::Value| {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(value).expect("run records encode"),
        )
        .map_err(|error| BenchmarkRefusal::Unreadable {
            detail: format!("{}: {error}", path.display()),
        })
    };
    let mut driven = 0usize;
    let mut index_generation: Option<String> = None;
    let mut repo_snapshot: Option<String> = None;
    for case in &manifest.cases {
        let objective_path = bench_root
            .join("objectives")
            .join(format!("{}.json", case.id));
        let objective_text = std::fs::read_to_string(&objective_path).map_err(|error| {
            BenchmarkRefusal::Unreadable {
                detail: format!("{}: {error}", objective_path.display()),
            }
        })?;
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct ObjectiveFile {
            case_id: String,
            objective: String,
        }
        let objective: ObjectiveFile = serde_json::from_str(&objective_text).map_err(|error| {
            BenchmarkRefusal::Unreadable {
                detail: format!("{}: {error}", objective_path.display()),
            }
        })?;
        if objective.case_id != case.id {
            return Err(BenchmarkRefusal::Unreadable {
                detail: format!(
                    "{}: carries caseId `{}`",
                    objective_path.display(),
                    objective.case_id
                ),
            });
        }

        let (evidence, _stripped) =
            oracle_evidence_paths(&bench_root.join("oracle").join(format!("{}.json", case.id)))?;

        let artifact_path = retrieval_root.join(format!("{}.json", case.id));
        let artifact_text = std::fs::read_to_string(&artifact_path).map_err(|error| {
            BenchmarkRefusal::Unreadable {
                detail: format!("{}: {error}", artifact_path.display()),
            }
        })?;
        let artifact: RetrievalArtifact =
            serde_json::from_str(&artifact_text).map_err(|error| BenchmarkRefusal::Unreadable {
                detail: format!("{}: {error}", artifact_path.display()),
            })?;
        if artifact.case_id != case.id {
            return Err(BenchmarkRefusal::Unreadable {
                detail: format!(
                    "{}: carries caseId `{}`",
                    artifact_path.display(),
                    artifact.case_id
                ),
            });
        }
        // The axes are held EQUAL across arms, so all cases must agree on the snapshots the run
        // declares -- twelve artifacts from two different index generations are two runs.
        match &index_generation {
            None => {
                index_generation = Some(artifact.index_generation.clone());
                if artifact.repo_snapshot != observed_tree {
                    return Err(BenchmarkRefusal::Unreadable {
                        detail: format!(
                            "the artifacts were produced against tree {} and the repo root is at \
                             {observed_tree}: coordinates from one source tree would slice \
                             another",
                            artifact.repo_snapshot
                        ),
                    });
                }
                repo_snapshot = Some(artifact.repo_snapshot.clone());
            }
            Some(generation) => {
                if *generation != artifact.index_generation
                    || repo_snapshot.as_deref() != Some(artifact.repo_snapshot.as_str())
                {
                    return Err(BenchmarkRefusal::Unreadable {
                        detail: format!(
                            "{}: index generation differs from the run's -- one run, one index",
                            artifact_path.display()
                        ),
                    });
                }
            }
        }

        let naive = naive_context(&objective.objective, &evidence, repo_root)?;
        let compiled = capsule_for_case(&objective.objective, &artifact, repo_root)?;
        let compiled_recall = evidence
            .iter()
            // Through the crate's canonical form, because THIS comparison is the recall gate:
            // two spellings of one file (`./src/lib.rs` and `src/lib.rs`) were reading as a miss,
            // and a miss here refuses the paired run. The bar that decides the spend was deciding
            // on textual equality of strings (Codex P1).
            .all(|path| {
                let wanted = graphhelm_development_benchmark::canonical_path(path);
                compiled
                    .evidence_paths
                    .iter()
                    .any(|have| graphhelm_development_benchmark::canonical_path(have) == wanted)
            });

        // Naive first, compiled second, per case -- the per-case arm order is part of the
        // protocol and it is fixed rather than recorded because it never varies.
        let capsule_prompt = String::from_utf8(compiled.capsule.clone()).map_err(|_| {
            BenchmarkRefusal::Unreadable {
                detail: format!("case {}: capsule is not UTF-8", case.id),
            }
        })?;
        let (naive_cost, compiled_cost) = match provider {
            Provider::Fake => (
                graphhelm_runtime::context_accounting::CostField::measured(
                    fake_input_tokens(naive.as_bytes()),
                    "fake-provider",
                ),
                graphhelm_runtime::context_accounting::CostField::measured(
                    fake_input_tokens(&compiled.capsule),
                    "fake-provider",
                ),
            ),
            Provider::Live {
                adapter,
                key,
                route_id,
                ..
            } => (
                live_arm_cost(adapter, key, &naive, 512, route_id)?,
                live_arm_cost(adapter, key, &capsule_prompt, 512, route_id)?,
            ),
        };
        write(
            &out_root.join("cases").join(format!("{}.json", case.id)),
            &serde_json::json!({
                "caseId": case.id,
                "baseline": {
                    "provider_reported_input_tokens": naive_cost,
                    "requiredEvidenceFound": true,
                },
                "compiled": {
                    "provider_reported_input_tokens": compiled_cost,
                    "requiredEvidenceFound": compiled_recall,
                },
            }),
        )?;
        driven += 1;
    }

    let route_label = match provider {
        Provider::Fake => "fake".to_owned(),
        Provider::Live { route_id, .. } => route_id.clone(),
    };
    // Derived from what the calls TRANSMIT, per provider -- an asserted literal on a held-equal
    // axis is a check that cannot fail (K on #531): both arms would copy one constant and
    // compare_arms would compare the constant to itself.
    let settings_label = transmitted_settings(provider_kind(provider), 512);
    let order: Vec<String> = manifest.cases.iter().map(|case| case.id.clone()).collect();
    let arm = serde_json::json!({
        "snapshot": repo_snapshot.clone().unwrap_or_else(|| "none".to_owned()),
        "indexSnapshot": index_generation.clone().unwrap_or_else(|| "none".to_owned()),
        "objective": "the frozen corpus, in manifest order",
        "permissions": ["read"],
        "modelRoute": route_label.clone(),
        "modelSettings": settings_label,
        "cleanState": true,
        "order": order,
        "seed": 7,
        "clock": arguments.clock,
        "budget": 512,
        "acceptanceContract": manifest.corpus_digest,
        "binaryDigest": arguments.binary_digest,
        "environment": arguments.environment,
        "cacheDiscipline": "cold",
    });

    // Last write: the eligibility marker. Everything before this is a partial run the reader
    // refuses by the absence of this file.
    write(
        &out_root.join("arms.json"),
        &serde_json::json!({ "baseline": arm, "compiled": arm }),
    )?;
    eprintln!(
        "run written: {driven} cases, route {route_label}, out {}",
        out_root.display()
    );
    Ok(())
}
