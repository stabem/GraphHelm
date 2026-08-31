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
    BenchmarkRefusal, Manifest, RetrievalArtifact, capsule_for_case, load_manifest, naive_context,
    oracle_evidence_paths, verify_frozen_files,
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
        }),
        _ => Err(usage()),
    }
}

fn run() -> i32 {
    let arguments = match parse() {
        Ok(arguments) => arguments,
        Err(code) => return code,
    };
    if arguments.provider != "fake" {
        eprintln!(
            "provider `{}` is not wired yet; `fake` is the only dry-run provider",
            arguments.provider
        );
        return 64;
    }

    match drive(&arguments) {
        Ok(()) => 0,
        Err(refusal) => {
            println!("{refusal:?}");
            2
        }
    }
}

/// The fake provider's whole contract: input tokens = ceil(bytes / 4), labelled as fake.
fn fake_input_tokens(context: &[u8]) -> u64 {
    (context.len() as u64).div_ceil(4)
}

fn drive(arguments: &Arguments) -> Result<(), BenchmarkRefusal> {
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
    let retrieval_root = std::path::Path::new(&arguments.retrieval);
    let out_root = std::path::Path::new(&arguments.out);

    // Everything is BUILT before anything is WRITTEN: a run directory must not come into
    // existence carrying half a run -- the reader treats missing cases as a partial corpus,
    // and it is right to.
    struct DrivenCase {
        id: String,
        naive_tokens: u64,
        compiled_tokens: u64,
        compiled_recall: bool,
    }
    let mut driven: Vec<DrivenCase> = Vec::with_capacity(manifest.cases.len());
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
            .all(|path| compiled.evidence_paths.contains(path));

        driven.push(DrivenCase {
            id: case.id.clone(),
            naive_tokens: fake_input_tokens(naive.as_bytes()),
            compiled_tokens: fake_input_tokens(&compiled.capsule),
            compiled_recall,
        });
    }

    let order: Vec<String> = manifest.cases.iter().map(|case| case.id.clone()).collect();
    let arm = serde_json::json!({
        "snapshot": repo_snapshot.clone().unwrap_or_else(|| "none".to_owned()),
        "indexSnapshot": index_generation.clone().unwrap_or_else(|| "none".to_owned()),
        "objective": "the frozen corpus, in manifest order",
        "permissions": ["read"],
        "modelRoute": "fake",
        "modelSettings": "temperature=0",
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
    write(
        &out_root.join("arms.json"),
        &serde_json::json!({ "baseline": arm, "compiled": arm }),
    )?;
    for case in &driven {
        write(
            &out_root.join("cases").join(format!("{}.json", case.id)),
            &serde_json::json!({
                "caseId": case.id,
                "baseline": {
                    "provider_reported_input_tokens": {
                        "value": case.naive_tokens,
                        "provenance": "measured",
                        "producer": "fake-provider",
                        "note": "",
                    },
                    "requiredEvidenceFound": true,
                },
                "compiled": {
                    "provider_reported_input_tokens": {
                        "value": case.compiled_tokens,
                        "provenance": "measured",
                        "producer": "fake-provider",
                        "note": "",
                    },
                    "requiredEvidenceFound": case.compiled_recall,
                },
            }),
        )?;
    }
    eprintln!(
        "run written: {} cases, provider fake, out {}",
        driven.len(),
        out_root.display()
    );
    Ok(())
}
