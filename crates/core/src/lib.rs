//! UI-independent core: data models, variable resolution, request building.
pub mod build;
pub mod dynamic;
pub mod encode;
pub mod env;
pub mod fuzzy;
pub mod json_tree;
pub mod lines;
pub mod meta;
pub mod models;
pub mod pretty;
pub mod vars;

pub use build::{build, build_template, BuildError, Built, BuiltBody, BuiltPart};
pub use dynamic::{dynamic_value, is_dynamic, DYNAMIC_VARIABLES};
pub use env::{to_map, EnvVar, Environment};
pub use fuzzy::fuzzy_score;
pub use json_tree::{CollapseState, JsonTree, JsonTreeError};
pub use meta::{CollectionMeta, Example, FolderMeta};
pub use models::*;
pub use vars::{Resolved, Variables};
