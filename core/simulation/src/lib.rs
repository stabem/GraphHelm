//! Effect-free deterministic simulation.

mod engine;
mod fixtures;

pub use engine::{SimulationError, SimulationResult, SimulationServices, simulate};
pub use fixtures::SimulationFixtures;
