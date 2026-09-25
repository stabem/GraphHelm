//! Regenerates `docs/acceptance/M05_ACCEPTANCE_MAP.md` from `m05-clauses.toml`.

fn main() {
    let root = acceptance_map::repo_root();
    let clauses = acceptance_map::load_clauses(&root);
    let rendered = acceptance_map::generate(&clauses);
    let target = root.join("docs/acceptance/M05_ACCEPTANCE_MAP.md");
    std::fs::write(&target, rendered).expect("the map writes");
    println!("wrote {}", target.display());
}
