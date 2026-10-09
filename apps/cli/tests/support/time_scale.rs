//! One knob for every wall-clock ceiling a CLI test puts on a child process (#549).
//!
//! **What went wrong without it.** Two runs of `cargo test -p graphhelm-cli` on one head (#533):
//! 29 failures in the first, 27 of which passed when their targets ran alone, and two OTHER cells
//! red in the second. Every one was a fixed wait on a child process (`graphhelm serve printed
//! nothing on stdout within 30s`, a 5 s bind refusal, a 5 s pipe readiness, a 5 s replay startup)
//! on a machine several lanes were building on. No cell failed twice.
//!
//! **What this is.** `GRAPHHELM_TEST_TIME_SCALE=<1..=20>` multiplies the ceilings that go through
//! [`scaled`]. Unset is 1: a developer's own run keeps every bound it had. A lane under the build
//! slot sets 3 (`docs/process/LANES.md`).
//!
//! **What this is not.** A ceiling here is a hang catcher: it says "this never happened", not
//! "this was fast enough". Scaling it cannot hide a defect, only delay its report. An assertion
//! that something took LESS than a bound, and any product budget, must not go through here: a
//! user's `graphhelm` never reads this variable.
//!
//! A value that does not parse panics rather than meaning 1: a lane that mistyped the knob would
//! otherwise read the same false reds and blame the machine.

use std::time::Duration;

pub const TIME_SCALE_ENV: &str = "GRAPHHELM_TEST_TIME_SCALE";
const MAX_FACTOR: u32 = 20;

/// The factor a raw value of the variable means. Pure, so the refusal is testable.
pub fn time_scale_factor(raw: Option<&str>) -> Result<u32, String> {
    let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Ok(1);
    };
    match raw.parse::<u32>() {
        Ok(factor) if (1..=MAX_FACTOR).contains(&factor) => Ok(factor),
        _ => Err(format!(
            "{TIME_SCALE_ENV}={raw:?} is not a whole number from 1 to {MAX_FACTOR}"
        )),
    }
}

/// `base` times the factor of this process's `GRAPHHELM_TEST_TIME_SCALE`.
pub fn scaled(base: Duration) -> Duration {
    let raw = std::env::var(TIME_SCALE_ENV).ok();
    base * time_scale_factor(raw.as_deref()).unwrap_or_else(|refusal| panic!("{refusal}"))
}
