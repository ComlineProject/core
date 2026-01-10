use comline_core::package::build::basic_storage::package::{check_difference, VersionBump};
use comline_core::schema::ir::compiler::interpreted::kind_search::{KindValue, Primitive};
use comline_core::schema::ir::frozen::unit::FrozenUnit;

fn make_field(name: &str, optional: bool) -> FrozenUnit {
    FrozenUnit::Field {
        docstring: None,
        parameters: vec![],
        optional,
        name: name.to_string(),
        kind_value: KindValue::Primitive(Primitive::S32(None)),
    }
}

fn make_struct(name: &str, fields: Vec<FrozenUnit>) -> FrozenUnit {
    FrozenUnit::Struct {
        docstring: None,
        parameters: vec![],
        name: name.to_string(),
        fields,
    }
}

fn make_schema(namespace: &str, items: Vec<FrozenUnit>) -> Vec<FrozenUnit> {
    let mut schema = vec![
        FrozenUnit::Namespace(namespace.to_string()),
    ];
    schema.extend(items);
    schema
}

#[test]
fn test_no_change() {
    let start = make_schema("test", vec![
        make_struct("Foo", vec![make_field("a", false)])
    ]);
    let end = start.clone();
    
    // Wrap in Vec of schemas
    let prev = vec![start];
    let next = vec![end];

    assert_eq!(check_difference(&prev, &next), VersionBump::None);
}

#[test]
fn test_struct_add_optional_field() {
    let start = make_schema("test", vec![
        make_struct("Foo", vec![make_field("a", false)])
    ]);
    let end = make_schema("test", vec![
        make_struct("Foo", vec![
            make_field("a", false),
            make_field("b", true) // Added Optional
        ])
    ]);

    let prev = vec![start];
    let next = vec![end];

    // Should be Minor
    assert_eq!(check_difference(&prev, &next), VersionBump::Minor);
}

#[test]
fn test_struct_add_required_field() {
    let start = make_schema("test", vec![
        make_struct("Foo", vec![make_field("a", false)])
    ]);
    let end = make_schema("test", vec![
        make_struct("Foo", vec![
            make_field("a", false),
            make_field("b", false) // Added Required
        ])
    ]);

    let prev = vec![start];
    let next = vec![end];

    // Should be Major
    assert_eq!(check_difference(&prev, &next), VersionBump::Major);
}

#[test]
fn test_struct_remove_field() {
    let start = make_schema("test", vec![
        make_struct("Foo", vec![
            make_field("a", false),
            make_field("b", true)
        ])
    ]);
    let end = make_schema("test", vec![
        make_struct("Foo", vec![make_field("a", false)])
    ]);

    let prev = vec![start];
    let next = vec![end];

    // Should be Major
    assert_eq!(check_difference(&prev, &next), VersionBump::Major);
}

#[test]
fn test_new_schema() {
    let prev = vec![];
    let next = vec![make_schema("test", vec![])];

    // New schema -> Minor
    assert_eq!(check_difference(&prev, &next), VersionBump::Minor);
}

#[test]
fn test_removed_schema() {
    let prev = vec![make_schema("test", vec![])];
    let next = vec![];

    // Removed schema -> Major
    assert_eq!(check_difference(&prev, &next), VersionBump::Major);
}
