//! Read-only inventory of supported local host configuration surfaces.

mod backup;
mod classification;
mod inventory;

pub use backup::{backup, backup_with_limit, valid_backup_id, verify_backup};
pub use classification::propose;
pub use inventory::inventory;
