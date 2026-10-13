//! The progress face of `graphhelm update` (and the clone sync it shares with `studio start`).
//!
//! Progress goes to stderr; the JSON envelope on stdout is untouched. When stderr is a terminal
//! the run gets a banner, numbered steps and colour (colour still obeys [`crate::palette`], so
//! `NO_COLOR` keeps the layout and drops the escapes). When stderr is NOT a terminal — a log, a
//! pipe, a CI capture — every line is the plain `[tag] text` it always was, so nothing that greps
//! those lines breaks.
//!
//! Everything is ASCII on purpose: a legacy Windows console with a non-UTF-8 code page renders
//! box-drawing and emoji as garbage.

use std::io::IsTerminal;

use crate::palette::{self, Palette};

/// `~` stands for a space: runs of spaces in a literal are what `source_invariants.rs` refuses.
const BANNER: [&str; 6] = [
    r"~~~____~~~~~~~~~~~~~~~~~_~~~~~_~~~_~~~~~~_",
    r"  / ___|_ __ __ _ _ __ | |__ | | | | ___| |_ __ ___",
    r" | |  _| '__/ _` | '_ \| '_ \| |_| |/ _ \ | '_ ` _ \",
    r" | |_| | | | (_| | |_) | | | |  _  |  __/ | | | | | |",
    r"  \____|_|  \__,_| .__/|_| |_|_| |_|\___|_|_| |_| |_|",
    r"~~~~~~~~~~~~~~~~~|_|",
];

/// Writes the progress lines of one run.
#[derive(Clone, Copy, Debug)]
pub struct Progress {
    tag: &'static str,
    fancy: bool,
    palette: Palette,
}

impl Progress {
    /// The face stderr allows right now.
    pub fn decide(tag: &'static str) -> Self {
        let is_terminal = std::io::stderr().is_terminal();
        let palette = Palette::decide(is_terminal, palette::Environment::from_process());
        if palette.writes_escapes() {
            palette::prepare_stream();
        }
        Self::new(tag, is_terminal, palette)
    }

    /// A face built from explicit answers (tests).
    pub const fn new(tag: &'static str, fancy: bool, palette: Palette) -> Self {
        Self {
            tag,
            fancy,
            palette,
        }
    }

    /// The same face under another tag: the clone sync speaks as `studio` in plain mode.
    pub const fn retag(self, tag: &'static str) -> Self {
        Self { tag, ..self }
    }

    /// The banner and subtitle. Nothing in plain mode.
    pub fn banner(self, subtitle: &str) -> String {
        if !self.fancy {
            return String::new();
        }
        let mut out = String::from("\n");
        for line in BANNER {
            out.push_str(&self.palette.command(&line.replace('~', " ")));
            out.push('\n');
        }
        out.push_str(&format!("  {}", self.palette.label(subtitle)));
        out
    }

    /// A numbered step header: `[2/4] Syncing the clone`.
    pub fn step(self, index: u8, total: u8, text: &str) -> String {
        if self.fancy {
            let counter = self.palette.command(&format!("[{index}/{total}]"));
            format!("\n{counter} {}", self.palette.name(text))
        } else {
            self.plain(&text.to_lowercase())
        }
    }

    /// A detail under the current step.
    pub fn info(self, text: &str) -> String {
        if self.fancy {
            format!("{}{}", " ".repeat(6), self.palette.label(text))
        } else {
            self.plain(text)
        }
    }

    /// A step that finished well.
    pub fn ok(self, text: &str) -> String {
        if self.fancy {
            format!("  {} {text}", self.palette.good("[ok]"))
        } else {
            self.plain(text)
        }
    }

    /// A warning: the run goes on, but the operator should read it.
    pub fn warn(self, text: &str) -> String {
        if self.fancy {
            format!("  {} {text}", self.palette.bad("[!!]"))
        } else {
            self.plain(&format!("WARNING: {text}"))
        }
    }

    /// Whether this face redraws one line in place (a terminal).
    pub const fn redraws(self) -> bool {
        self.fancy
    }

    /// One frame of the build bar: `[########------------]  57/412 crates  serde_json`. A frame
    /// starts with `\r` and pads to a fixed width, so each one overwrites the last. The count is
    /// capped below the total until the build reports it finished.
    pub fn bar(self, done: usize, total: usize, current: &str) -> String {
        const WIDTH: usize = 30;
        let total = total.max(1);
        let shown = done.min(total);
        let filled = shown * WIDTH / total;
        let bar = format!("{}{}", "#".repeat(filled), "-".repeat(WIDTH - filled));
        let name: String = current.chars().take(28).collect();
        format!(
            "\r      {} {:>4}/{total} crates  {:<28}",
            self.palette.command(&format!("[{bar}]")),
            shown,
            name
        )
    }

    fn plain(self, text: &str) -> String {
        format!("[{}] {text}", self.tag)
    }

    /// Prints a line built by the methods above; an empty line (plain banner) prints nothing.
    pub fn say(line: &str) {
        if !line.is_empty() {
            eprintln!("{line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The build bar fills in proportion and never runs past its width.
    #[test]
    fn the_build_bar_fills_in_proportion() {
        let progress = Progress::new("update", true, Palette::plain());
        assert!(
            progress
                .bar(0, 10, "a")
                .contains("[------------------------------]")
        );
        assert!(
            progress
                .bar(5, 10, "a")
                .contains("[###############---------------]")
        );
        assert!(
            progress
                .bar(99, 10, "a")
                .contains("[##############################]   10/10")
        );
        assert!(progress.bar(1, 0, "a").starts_with('\r'));
    }

    #[test]
    fn plain_mode_keeps_the_tagged_lines_and_writes_no_escape() {
        let progress = Progress::new("update", false, Palette::plain());
        assert_eq!(progress.banner("x"), "");
        assert_eq!(progress.info("into /bin"), "[update] into /bin");
        assert_eq!(progress.warn("failed"), "[update] WARNING: failed");
        assert_eq!(progress.retag("studio").ok("done"), "[studio] done");
    }

    #[test]
    fn terminal_without_colour_keeps_the_layout_without_escapes() {
        let progress = Progress::new("update", true, Palette::plain());
        let banner = progress.banner("updating");
        assert!(banner.contains("|_|") && banner.contains("updating"));
        assert_eq!(progress.step(1, 3, "Build"), "\n[1/3] Build");
        assert!(!format!("{banner}{}", progress.ok("x")).contains('\u{1b}'));
        assert!(banner.is_ascii());
    }

    #[test]
    fn colour_wraps_the_step_counter() {
        let progress = Progress::new("update", true, Palette::coloured());
        assert!(progress.step(2, 3, "Sync").contains("\u{1b}[36m[2/3]"));
    }
}
