// Standard Uses

// Local Uses

// External Uses
use comline_core::autodoc;
use comline_core::schema::ir::compiler::interpreted::kind_search::{KindValue, Primitive};
use comline_core::schema::ir::frozen::unit::FrozenUnit;

use once_cell::sync::Lazy;


static PREVIOUS_FROZEN_UNITS: Lazy<Vec<FrozenUnit>> = Lazy::new(||
    vec![
        FrozenUnit::Namespace("foobar".to_owned()),
        FrozenUnit::Constant {
            docstring: None,
            name: "DUCKS".to_string(), kind_value: KindValue::Primitive(Primitive::U8(Some(10))),
        }
    ]
);

static CURRENT_FROZEN_UNITS: Lazy<Vec<FrozenUnit>> = Lazy::new(||
    vec![
        FrozenUnit::Namespace("foo::bar".to_owned()),
        FrozenUnit::Constant {
            docstring: None,
            name: "DUCKS".to_string(), kind_value: KindValue::Primitive(Primitive::U8(Some(15))),
        }
    ]
);

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
