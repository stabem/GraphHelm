//! Offline fixture for #549: a "native runtime" whose `--version` answers only after the number of
//! milliseconds written in the adjacent `<exe>.delay` file, then exits 0. It stands for a real
//! host CLI that a loaded machine starts slowly; it reads no environment (the probe clears it).
fn main() {
    let delay = std::fs::read_to_string(std::env::current_exe().unwrap().with_extension("delay"))
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok())
        .unwrap_or(0);
    std::thread::sleep(std::time::Duration::from_millis(delay));
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("slow-version 1.0.0");
    } else {
        std::process::exit(2);
    }
}
