//! Dev-only helpers shared by the workspace's source guards (#1315). Never a runtime dependency:
//! every adopter lists it under `[dev-dependencies]`.

pub mod detect;
pub mod walk;
