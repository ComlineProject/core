// Standard Uses
use std::io;

// Crate Uses

// External Uses
use snafu::Snafu;


#[repr(u16)]
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum DiffError {
    // #[strum(props(Id="2"))]
    #[snafu(display("Found namespace '{target}' when its already declared in '{origin}'"))]
    Namespace { origin: String, target: String },

    // #[strum(props(Id="2"))]
    #[snafu(display("Spec version was not assigned'"))]
    // SpecVersionNotTold { source: Box<dyn std::error::Error + Send + Sync> }
    SpecVersionNotTold { source: io::Error },

    // #[strum(props(Id="2"))]
    #[snafu(display("Schema paths list is empty, at least one schema is required'"))]
    // SpecVersionNotTold { source: Box<dyn std::error::Error + Send + Sync> }
    SchemaPathsEmpty,
}


// pub type CompileResult<T, E = CompileError> = std::result::Result<T, E>;
