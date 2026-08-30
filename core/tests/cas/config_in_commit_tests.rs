// The frozen congregation is recorded in each commit's root tree (reproducibility).

use std::fs;

use comline_core::package::build::build;
use comline_core::package::build::cas::objects::{Commit, EntryMode, Tree};
use comline_core::package::build::cas::{refs, ObjectStore};
use comline_core::package::config::ir::frozen::{cas::blob, FrozenUnit};
use tempfile::TempDir;

const CONFIG: &str = "\
congregation demo
specification_version = 1

code_generation = {
    languages = {
        rust#1.70.0 = {}
    }
}
";

fn scaffold(root: &std::path::Path, config: &str) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("config.idp"), config).unwrap();
    fs::write(root.join("src/main.ids"), "struct Ping {\n    seq: u32\n}\n").unwrap();
}

fn root_tree(root: &std::path::Path) -> (ObjectStore, Tree) {
    let store = ObjectStore::new(root);
    let head = refs::read_ref(root, refs::main_ref()).unwrap();
    let commit = Commit::from_bytes(&store.read(&head).unwrap()).unwrap();
    let tree = Tree::from_bytes(&store.read(&commit.tree).unwrap()).unwrap();
    (store, tree)
}

#[test]
fn build_records_the_frozen_config_in_the_commit_tree() {
    let dir = TempDir::new().unwrap();
    scaffold(dir.path(), CONFIG);

    build(dir.path()).expect("build");

    let (store, tree) = root_tree(dir.path());
    let entry = tree
        .entries
        .iter()
        .find(|e| e.name == "config")
        .expect("root tree has a `config` entry");
    assert_eq!(entry.mode, EntryMode::Blob);

    let units = blob::read_config(&store, &entry.hash).unwrap();
    assert!(
        units
            .iter()
            .any(|u| matches!(u, FrozenUnit::SpecificationVersion(1))),
        "frozen config carries specification_version"
    );
    assert!(
        units
            .iter()
            .any(|u| matches!(u, FrozenUnit::CodeGeneration(_))),
        "frozen config carries the code_generation declaration"
    );
}

#[test]
fn a_config_only_change_commits_without_bumping_the_version() {
    let dir = TempDir::new().unwrap();
    scaffold(dir.path(), CONFIG);

    let v1 = build(dir.path()).unwrap().current_version;
    let head1 = refs::read_ref(dir.path(), refs::main_ref()).unwrap();

    // Add a declared language — a config-only change, no schema edit.
    scaffold(
        dir.path(),
        &CONFIG.replace("rust#1.70.0 = {}", "rust#1.70.0 = {}\n        python#3.11.0 = {}"),
    );
    let res = build(dir.path()).unwrap();
    let head2 = refs::read_ref(dir.path(), refs::main_ref()).unwrap();

    assert_ne!(head1, head2, "a config change creates a new commit");
    assert_eq!(
        res.current_version, v1,
        "a config-only change does not bump the version yet (semantics are a follow-up)"
    );
}
