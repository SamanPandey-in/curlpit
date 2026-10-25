//! UI-independent core: data models, variable resolution, request building.
pub mod build;
pub mod encode;
pub mod models;
pub mod vars;

pub use build::{build, BuildError, Built, BuiltBody, BuiltPart};
pub use models::*;
pub use vars::{Resolved, Variables};
