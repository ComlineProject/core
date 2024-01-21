// Standard Uses

// Crate Uses
use crate::report::ReportDetails;

// External Uses
use snafu::Snafu;


#[repr(u16)]
#[derive(Debug, Snafu)]
pub enum CompileError {
    #[snafu(display(
        "Found namespace '{target}' when its already declared in '{origin}'"
    ))]
    NamespaceCollision { origin: String, target: String } = 0,

    #[snafu(display("No type found by name '{name}'"))]
    TypeNotFound { name: String },
}

impl CompileError {
    pub(crate) fn code(&self) -> u16 {
        unsafe { *<*const _>::from(self).cast::<u16>() }
    }
}


#[repr(u16)]
#[derive(Debug, Snafu)]
pub enum CompileWarning {
    #[snafu(
        display("No dependencies were specified, this will automatically add comline lang_lib'")
    )]
    DependenciesNotFilled
}

impl CompileWarning {
    pub(crate) fn code(&self) -> u16 {
        unsafe { *<*const _>::from(self).cast::<u16>() }
    }
}


#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum Report {
    #[snafu(display(
        "Error[E0{}]: {source}\nAt line {}:{} (position {}:{})",
        source.code(), details.line.start, details.line.end, details.pos.start, details.pos.end
    ))]
    CompileErrorError {
        source: CompileError,
        details: ReportDetails,
    },

    #[snafu(display(
        "Warning[W0{}]: {source}\nAt line {}:{} (position {}:{})",
        source.code(), details.line.start, details.line.end, details.pos.start, details.pos.end
    ))]
    CompileWarningError {
        source: CompileWarning,
        details: ReportDetails,
    },
}
