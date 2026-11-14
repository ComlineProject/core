// `std` is built in and never vendored, so a build has no `dep_std`
// entry, yet its compiled schemas are in every build. It's compared by
// reach, like a vendored dependency: what the package doesn't reach of it
// is neither the package's change nor a version bump.

use comline_core::package::build::cas::objects::{EntryMode, Tree};
use comline_core::package::build::cas::ObjectStore;
use comline_core::package::build::VersionBump;
use tempfile::TempDir;

/// A built version's root tree holding `schemas`, each as a `schema_<i>`
/// entry, the way a build writes them.
fn tree_of(store: &ObjectStore, schemas: &[Vec<comline_core::schema::ir::frozen::unit::FrozenUnit>]) -> Tree {
    use comline_core::schema::ir::frozen::cas::blob::build_tree_from_schema;
    let mut root = Tree::new();
    for (i, schema) in schemas.iter().enumerate() {
        let tree = build_tree_from_schema(schema, store).unwrap();
        let hash = store.write(&tree.to_bytes().unwrap()).unwrap();
        root.add_entry(EntryMode::Tree, format!("schema_{i}"), hash);
    }
    root
}

#[test]
fn std_is_compared_by_reach_like_a_dependency() {
    // `std` is built in, never vendored (no `dep_std` entry), yet its
    // compiled schemas are in every build. A declaration `std` gains that
    // the package doesn't reach is neither the package's own feature nor a
    // version bump.
    use comline_core::package::build::cas::build::compare;
    use comline_core::schema::ir::frozen::unit::FrozenUnit;

    let unit = |name: &str| FrozenUnit::Struct {
        docstring: None,
        parameters: vec![],
        name: name.to_string(),
        fields: vec![],
        span: (0, 0),
    };
    let own = vec![FrozenUnit::Namespace("api".into()), unit("Holder")];
    let std_before = vec![FrozenUnit::Namespace("std::extra".into()), unit("Existing")];
    let std_after = vec![FrozenUnit::Namespace("std::extra".into()), unit("Existing"), unit("Added")];

    let dir = TempDir::new().unwrap();
    let store = ObjectStore::new(dir.path());
    store.init().unwrap();
    let old = tree_of(&store, &[own.clone(), std_before]);
    let new = tree_of(&store, &[own, std_after]);

    let comparison = compare(&store, &old, &new).unwrap();
    assert!(comparison.schema_changes.is_empty(), "{:?}", comparison.schema_changes);
    assert!(comparison.dependency_changes.is_empty(), "{:?}", comparison.dependency_changes);
    // Trees with no stored config count it unseen, at most a patch.
    assert!(comparison.bump < VersionBump::Minor, "{:?}", comparison.bump);
}
