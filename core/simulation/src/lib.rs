//! Effect-free deterministic simulation.

mod engine;
mod executor;
mod fixtures;

pub use engine::{SimulationError, SimulationResult, SimulationServices, simulate};
pub use executor::FixtureExecutor;
pub use fixtures::SimulationFixtures;
