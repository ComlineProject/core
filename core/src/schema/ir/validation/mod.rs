pub mod symbols;
pub mod validator;

use crate::diagnostics::Diagnostic;
use crate::schema::ir::frozen::unit::FrozenUnit;

/// Validate a set of declarations (FrozenUnits). `Ok` carries any non-fatal
/// warnings produced along the way (e.g. a reference to a `@deprecated`
/// struct/error) - empty when there's nothing to warn about.
pub fn validate(units: &[FrozenUnit]) -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
    validator::validate(units)
}
