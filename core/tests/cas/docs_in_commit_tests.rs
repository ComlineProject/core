// The `//!` headers are frozen in each commit's root tree (core#145), so a
// built version documents with them.

use std::fs;

use comline_core::doc::{self, FrozenDocs, DOCS_ENTRY};
use comline_core::package::build::build;
use comline_core::package::build::cas::objects::{Commit, Tree};
use comline_core::package::build::cas::{refs, ObjectStore};
use comline_core::schema::ir::frozen::cas::blob::load_schema_from_tree;
use tempfile::TempDir;

const CONFIG: &str = "\
//! The demo package.
//!
//! Second paragraph.

congregation demo
specification_version = 1
";

fn root_tree(root: &std::path::Path) -> (ObjectStore, Commit, Tree) {
    let store = ObjectStore::new(root);
    let head = refs::read_ref(root, refs::main_ref()).unwrap();
    let commit = Commit::from_bytes(&store.read(&head).unwrap()).unwrap();
    let tree = Tree::from_bytes(&store.read(&commit.tree).unwrap()).unwrap();
    (store, commit, tree)
}

#[test]
fn a_build_freezes_the_headers_and_a_built_version_documents_with_them() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("config.idp"), CONFIG).unwrap();
    fs::write(
        dir.path().join("src/api.ids"),
        "//! The API.\n\n/// A ping.\nstruct Ping {\n    seq: u32\n}\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/bare.ids"), "struct Pong {\n    seq: u32\n}\n").unwrap();

    build(dir.path()).expect("build");

    // Change the sources: the frozen version must not read them.
    fs::write(dir.path().join("config.idp"), "congregation demo\nspecification_version = 1\n").unwrap();
    fs::write(dir.path().join("src/api.ids"), "struct Ping {\n    seq: u32\n}\n").unwrap();

    let (store, commit, tree) = root_tree(dir.path());
    let headers = doc::read_docs(&store, &tree).unwrap();
    assert_eq!(
        headers,
        FrozenDocs {
            package: Some("The demo package.\n\nSecond paragraph.".into()),
            modules: [("api".to_string(), "The API.".to_string())].into(),
        }
    );

    let schemas: Vec<_> = tree
        .entries
        .iter()
        .filter(|e| e.name.starts_with("schema_"))
        .map(|e| {
            let sub = Tree::from_bytes(&store.read(&e.hash).unwrap()).unwrap();
            load_schema_from_tree(&store, &sub).unwrap()
        })
        .collect();
    let model = doc::model_frozen("demo", Some(commit.version.clone()), &schemas, &doc::frozen_dependencies(&tree), headers);
    assert_eq!(model.package.docs.as_deref(), Some("The demo package.\n\nSecond paragraph."));
    let api = model.modules.iter().find(|m| m.path == "api").unwrap();
    assert_eq!(api.docs.as_deref(), Some("The API."));
    assert!(matches!(&api.units[0], doc::Unit::Struct { name, docs: Some(d), .. }
        if name == "Ping" && d.text == "A ping."));
    let bare = model.modules.iter().find(|m| m.path == "bare").unwrap();
    assert_eq!(bare.docs, None);
}

#[test]
fn a_package_without_headers_stores_no_docs_entry() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("config.idp"), "congregation demo\nspecification_version = 1\n").unwrap();
    fs::write(dir.path().join("src/api.ids"), "struct Ping {\n    seq: u32\n}\n").unwrap();

    build(dir.path()).expect("build");

    let (store, _, tree) = root_tree(dir.path());
    assert!(tree.entries.iter().all(|e| e.name != DOCS_ENTRY));
    assert_eq!(doc::read_docs(&store, &tree).unwrap(), FrozenDocs::default());
}
