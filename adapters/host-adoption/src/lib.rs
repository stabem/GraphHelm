//! Read-only inventory of supported local host configuration surfaces.

mod apply;
mod backup;
mod classification;
pub mod hosts;
mod inventory;
mod journal;
mod observation;
mod ownership;
pub use observation::{verify_activation, verify_activation_at};
mod restore;
mod surfaces;

pub use apply::{apply, apply_with_packages, is_graphhelm_registration, recover, root_bindings};
pub use backup::{backup, backup_with_limit, valid_backup_id, verify_backup};
pub use classification::{Resolution, propose, redact, resolve, seal, write_private};
pub use inventory::inventory;
pub use restore::{apply_restore, plan_restore};
mod storage;
