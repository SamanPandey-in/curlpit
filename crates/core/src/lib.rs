//! UI-independent core: data models, variable resolution, request building.
pub mod build;
pub mod encode;
pub mod env;
pub mod json_tree;
pub mod lines;
pub mod models;
pub mod pretty;
pub mod vars;

pub use build::{build, build_template, BuildError, Built, BuiltBody, BuiltPart};
pub use env::{to_map, EnvVar, Environment};
pub use json_tree::{CollapseState, JsonTree, JsonTreeError};
pub use models::*;
pub use vars::{Resolved, Variables};
