// TODO: LALRPOP generated code clashes with this, find an alternative to reintroduce
//       or maybe consider moving ast parsing to it's own crate
//#![deny(rust_2018_idioms)]

// Relative Modules
pub mod config_value;
pub mod diagnostics;
pub mod encodings;
pub mod schema;
pub mod package;
#[cfg(feature = "compiler")]
pub mod autodoc;
pub mod settings;
pub mod utils;
// Code generation moved to `ComlineProject/generation` (`comline-codelib-gen`);
// the CLI is the composition root. See design/generation.md.
