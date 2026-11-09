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
    fs::write(root.join("src/api.ids"), "struct Ping {\n    seq: u32\n}\n").unwrap();
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
    // A new build, so the next dev count; but nothing calls for a bump.
    assert_eq!(v1, "0.1.0-dev.1");
    assert_eq!(res.current_version, "0.1.0-dev.2");
    assert_eq!(res.version_bump, comline_core::package::build::VersionBump::None);
    assert_eq!(res.release_bump, comline_core::package::build::VersionBump::None);
    assert!(
        res.config_changes.is_empty(),
        "code_generation is not a versioned config unit"
    );
}

#[test]
fn a_package_without_a_known_edition_does_not_build() {
    // Every package names its edition (core#102); edition 1 is the only one.
    // Moving between editions bumps nothing by itself, which the config
    // diff's own tests cover.
    for (config, expected) in [
        (CONFIG.replace("specification_version = 1\n", ""), "edition::missing"),
        (CONFIG.replace("specification_version = 1", "specification_version = 2"), "edition::unknown"),
    ] {
        let dir = TempDir::new().unwrap();
        scaffold(dir.path(), &config);
        let err = build(dir.path()).expect_err("no known edition");
        let text = format!("{err:#}");
        assert!(text.contains(expected), "{expected}: {text}");
        assert!(text.contains("edition 1"), "the help names edition 1: {text}");
    }
}
