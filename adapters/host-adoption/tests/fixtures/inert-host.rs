//! Offline fixture executable; its adjacent text input controls probes without running any hook.
fn main() {
    let input = std::fs::read_to_string(std::env::current_exe().unwrap().with_extension("input")).unwrap();
    if let Some(path) = input.lines().nth(3).filter(|p| !p.is_empty()) { std::fs::write(path, b"probe started").unwrap(); }
    // #549: an optional fifth line delays every answer, as a loaded machine delays a real host.
    if let Some(ms) = input.lines().nth(4).and_then(|d| d.trim().parse::<u64>().ok()) { std::thread::sleep(std::time::Duration::from_millis(ms)); }
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => println!("{}", input.lines().next().unwrap()),
        Some("--help") => {
            if let Some(path) = input.lines().nth(2).filter(|p| !p.is_empty()) { std::fs::write(path, b"drift").unwrap(); }
            println!("{}", input.lines().nth(1).unwrap_or(""));
        },
        _ => std::process::exit(2),
    }
}
