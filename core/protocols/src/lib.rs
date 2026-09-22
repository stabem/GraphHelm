//! Stable GraphHelm wire contracts.

mod actor;
pub mod adoption;
// `#[macro_use]` so `wire_vocabulary!` reaches the modules declared after this one. It was
// module-private while `development` was its only user; `simulation` adopting it for `NodeState`
// (#408) is what needed the scope. Declaration order is the visibility rule for `macro_rules!`,
// so this line must stay above every module that uses it.
#[macro_use]
mod development;
mod diagnostic;
mod draft;
mod event;
mod graph;
mod persistence;
mod policy;
mod projection;
mod simulation;

pub use actor::*;
pub use development::*;
pub use diagnostic::*;
pub use draft::*;
pub use event::*;
pub use graph::*;
pub use persistence::*;
pub use policy::*;
pub use projection::*;
pub use simulation::*;
