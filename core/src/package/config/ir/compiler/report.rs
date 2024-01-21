// Standard Uses
use std::io;

// Crate Uses

// External Uses
use snafu::Snafu;


#[repr(u16)]
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum CompileError {
    #[snafu(display("Found namespace '{target}' when its already declared in '{origin}'"))]
    Namespace { origin: String, target: String } = 0,

    #[snafu(display("Spec version was not assigned'"))]
    // SpecVersionNotTold { source: Box<dyn std::error::Error + Send + Sync> }
    SpecVersionNotTold { source: io::Error },

    #[snafu(display("Schema paths list is empty, at least one schema is required'"))]
    // SpecVersionNotTold { source: Box<dyn std::error::Error + Send + Sync> }
    SchemaPathsEmpty,
}

/*
impl From<CompileError> for Error {
    fn from(e: CompilerError) -> Box<Self> {
        super::Error::IO {
            source: Box::new(e),
        }
    }
}
*/

pub type CompileResult<T, E = CompileError> = Result<T, E>;
