// Relative Modules
pub mod document;
pub mod package;
pub mod schema;

// Standard Uses

// Crate Uses
use crate::autodoc::schema::Document;
use crate::schema::ir::diff;
use crate::schema::ir::diff::Differ;
use crate::schema::ir::frozen::unit::FrozenUnit;

// External Uses

// FIXME: This function uses an old diff API that no longer exists
// The autodoc system is incomplete (has todo!() markers) and not currently used
// Commenting out to allow compilation - will need refactoring when autodoc is implemented
/*
#[allow(unused)]
pub fn document_differences(from: &Vec<FrozenUnit>, to: &Vec<FrozenUnit>) -> Document {
    let mut listeners: Vec<Box<dyn Differ>> = vec![Document::new()];

    diff::match_differ(&mut listeners, from, to);

    *listeners.remove(0).downcast::<Document>().unwrap()
}
*/

/*
#[allow(unused)]
pub fn node_difference(from: FrozenUnit, to: FrozenUnit) {
    for node in from {
        match node {
            FrozenUnit::Namespace(n) => {}
            FrozenUnit::Import(_) => {}
            FrozenUnit::Constant { .. } => {}
            FrozenUnit::Property { .. } => {}
            FrozenUnit::Parameter { .. } => {}
            FrozenUnit::ExpressionBlock { .. } => {}
            FrozenUnit::Enum { .. } => {}
            FrozenUnit::EnumVariant(_) => {}
            FrozenUnit::Settings { .. } => {}
            FrozenUnit::Struct { .. } => {}
            FrozenUnit::Protocol { .. } => {}
            FrozenUnit::Function { .. } => {}
            FrozenUnit::Error { .. } => {}
            FrozenUnit::Validator { .. } => {}
            FrozenUnit::Field { .. } => {}
        }
    }
}
*/
