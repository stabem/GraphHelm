//! `graphhelm gate …` — commands beside the CI gate's own run. The gate's verdict is
//! `ci/gate.ps1`'s and stays deterministic; nothing under this module changes a colour, selects
//! a stage, counts a pass or re-queues a head.

pub(crate) mod classify_red;
