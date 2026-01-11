// Tests for schema integration (blob.rs)

use comline_core::package::build::cas::ObjectStore;
use comline_core::schema::ir::compiler::kind_search::{KindValue, Primitive};
use comline_core::schema::ir::frozen::cas::blob::{
    blob_to_frozen_unit, build_tree_from_schema, frozen_unit_to_blob,
    load_schema_from_tree,
};
use comline_core::schema::ir::frozen::unit::FrozenUnit;
use tempfile::TempDir;

#[test]
fn test_frozen_unit_blob_roundtrip() {
    let unit = FrozenUnit::Constant {
        docstring: None,
        name: "TEST_CONST".to_string(),
        kind_value: KindValue::Primitive(Primitive::U32(Some(42))),
    };

    let blob = frozen_unit_to_blob(&unit).unwrap();
    let restored = blob_to_frozen_unit(&blob).unwrap();

    match (&unit, &restored) {
        (
            FrozenUnit::Constant { name: n1, .. },
            FrozenUnit::Constant { name: n2, .. },
        ) => assert_eq!(n1, n2),
        _ => panic!("Type mismatch"),
    }
}

#[test]
fn test_build_tree_from_schema() {
    let temp_dir = TempDir::new().unwrap();
    let store = ObjectStore::new(temp_dir.path());
    store.init().unwrap();

    let schema = vec![
        FrozenUnit::Struct {
            docstring: None,
            name: "User".to_string(),
            fields: vec![],
            impls: vec![],
        },
        FrozenUnit::Enum {
            docstring: None,
            name: "Status".to_string(),
            variants: vec![],
        },
    ];

    let tree = build_tree_from_schema(&schema, &store).unwrap();

    assert_eq!(tree.entries.len(), 2);
    assert!(tree.entries[0].name.contains("User"));
    assert!(tree.entries[1].name.contains("Status"));
}

#[test]
fn test_load_schema_from_tree() {
    let temp_dir = TempDir::new().unwrap();
    let store = ObjectStore::new(temp_dir.path());
    store.init().unwrap();

    let original_schema = vec![FrozenUnit::Constant {
        docstring: None,
        name: "VERSION".to_string(),
        kind_value: KindValue::Primitive(Primitive::String(Some("1.0.0".to_string()))),
    }];

    // Build tree and write to store
    let tree = build_tree_from_schema(&original_schema, &store).unwrap();

    // Load schema back from tree
    let loaded_schema = load_schema_from_tree(&store, &tree).unwrap();

    assert_eq!(loaded_schema.len(), 1);
    match &loaded_schema[0] {
        FrozenUnit::Constant { name, .. } => assert_eq!(name, "VERSION"),
        _ => panic!("Wrong unit type"),
    }
}
