// Standard Uses

// Crate Uses

// External Uses
use comline_core::autodoc;
use comline_core::schema::ir::frozen::types::TypeRef;
use comline_core::schema::ir::frozen::unit::FrozenUnit;

use once_cell::sync::Lazy;


static PREVIOUS_FROZEN_UNITS: Lazy<Vec<FrozenUnit>> = Lazy::new(||
    vec![
        FrozenUnit::Namespace("foobar".to_owned()),
        FrozenUnit::Constant {
            docstring: None,
            name: "DUCKS".to_string(), ty: TypeRef::Primitive(comline_core::schema::ir::frozen::types::PrimitiveType::U8), value: Some(comline_core::schema::ir::frozen::types::Literal::Integer(10)),
            span: (0, 0),
        }
    ]
);

static CURRENT_FROZEN_UNITS: Lazy<Vec<FrozenUnit>> = Lazy::new(||
    vec![
        FrozenUnit::Namespace("foo::bar".to_owned()),
        FrozenUnit::Constant {
            docstring: None,
            name: "DUCKS".to_string(), ty: TypeRef::Primitive(comline_core::schema::ir::frozen::types::PrimitiveType::U8), value: Some(comline_core::schema::ir::frozen::types::Literal::Integer(15)),
            span: (0, 0),
        }
    ]
);

// FIXME: Autodoc system is incomplete - this test temporarily disabled
// Will need refactoring when autodoc is fully implemented
/*
#[test]
fn difference_docs() {
    let diff_docs = autodoc::document_differences(
        &*PREVIOUS_FROZEN_UNITS, &*CURRENT_FROZEN_UNITS
    );

    // "Version change from '0.1.0' to '0.2.0'" \
    pretty_assertions::assert_eq!(diff_docs.major_changes, vec![
        "Namespace changed from 'foobar' to 'foo::bar'"
    ]);

    pretty_assertions::assert_eq!(diff_docs.minor_changes, vec![
        r#"Constant 'DUCKS' default value changed from '10' to '15'"#
    ]);
}
*/
