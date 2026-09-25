//! Records a demonstration manifest over an already-run store (M06 Task 6).
//!
//! `record_demonstration <demo-dir> <stream-id> <seed>` — the SEED IS HANDED IN: it is
//! sampled at recording time from entropy the operator (or the gate harness) injects,
//! never generated here, and it is frozen into `demo.json` forever. The tool derives the
//! node-check traversal from the seed, replays the store with the current build to record
//! the projection digest, writes `demo.json`, and rewrites `SHA256SUMS` over every file
//! in the directory — hash the bytes as written; commit with `.gitattributes -text`
//! already covering the tree (the 05f CRLF lesson).

use std::path::Path;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(directory), Some(stream_id), Some(seed)) = (args.next(), args.next(), args.next())
    else {
        eprintln!("usage: record_demonstration <demo-dir> <stream-id> <seed>");
        std::process::exit(2);
    };
    let seed: u64 = seed.parse().expect("the seed is a u64");
    let base = Path::new(&directory);

    let projection = acceptance_map::replay_demonstration_store(&base.join("events"), &stream_id)
        .expect("the recorded store replays");
    let names: Vec<String> = projection.node_states.keys().cloned().collect();
    let order = acceptance_map::seed_traversal(seed, &names);
    let node_checks: Vec<acceptance_map::DemoNodeCheck> = order
        .into_iter()
        .map(|node| {
            let state = projection
                .node_states
                .get(&node)
                .and_then(|state| serde_json::to_value(state).ok())
                .and_then(|value| value.as_str().map(str::to_owned))
                .expect("a terminal state serializes");
            acceptance_map::DemoNodeCheck { node, state }
        })
        .collect();
    let manifest = acceptance_map::DemoManifest {
        seed,
        stream_id,
        node_checks,
        expected_projection_digest: acceptance_map::projection_digest(&projection),
    };
    let rendered = serde_json::to_string_pretty(&manifest).expect("the manifest serializes");
    std::fs::write(base.join("demo.json"), format!("{rendered}\n")).expect("demo.json writes");

    // SHA256SUMS over every file (except itself), bytes as written.
    use sha2::{Digest, Sha256};
    let mut files = Vec::new();
    walk(base, base, &mut files);
    files.sort();
    let mut sums = String::new();
    for relative in files {
        if relative == "SHA256SUMS" {
            continue;
        }
        let bytes = std::fs::read(base.join(&relative)).expect("an artifact file reads");
        sums.push_str(&format!(
            "{}  {relative}\n",
            hex::encode(Sha256::digest(bytes))
        ));
    }
    std::fs::write(base.join("SHA256SUMS"), sums).expect("SHA256SUMS writes");
    println!(
        "recorded {} (seed {seed}, digest {})",
        base.display(),
        manifest.expected_projection_digest
    );
}

fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, base, out);
        } else if let Ok(relative) = path.strip_prefix(base) {
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}
