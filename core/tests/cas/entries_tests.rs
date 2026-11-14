// The entries the IR migrations framework reserves (core#268, step 1.2):
// `unknown` and `produced_by`. Nothing writes them yet, so these trees are
// made by hand; what is tested is that every reader fails closed.

use comline_core::doc;
use comline_core::package::build::cas::entries::{
    read_produced_by, read_unknown, require_known, VersionHasUnknowns,
};
use comline_core::package::build::cas::objects::{Blob, EntryMode, Tree};
use comline_core::package::build::cas::storage::to_cbor;
use comline_core::package::build::cas::ObjectStore;
use comline_ir_formats::v3::types::{ProducedBy, UnknownRecord};
use serde_derive::Serialize;
use tempfile::TempDir;

fn store() -> (TempDir, ObjectStore) {
    let dir = TempDir::new().unwrap();
    let store = ObjectStore::new(dir.path());
    store.init().unwrap();
    (dir, store)
}

/// A root tree with one entry, `name`, of `mode`, holding the blob `content`.
fn tree_with(store: &ObjectStore, name: &str, mode: EntryMode, content: &[u8]) -> Tree {
    let hash = store
        .write(&Blob::new(content.to_vec()).to_bytes().unwrap())
        .unwrap();
    let mut tree = Tree::new();
    tree.add_entry(mode, name.to_string(), hash);
    tree
}

fn record() -> UnknownRecord {
    UnknownRecord {
        path: "/schemas/0/units/4/ty".into(),
        step: "3-4".into(),
        reason: "format 3 froze `f32` as `float`".into(),
        original: Some(b"float".to_vec()),
    }
}

fn unknown_entry(store: &ObjectStore, records: &[UnknownRecord]) -> Tree {
    tree_with(
        store,
        "unknown",
        EntryMode::Blob,
        &to_cbor(records).unwrap(),
    )
}

#[test]
fn a_version_without_the_entries_has_no_unknowns_and_an_unknown_writer() {
    let (_dir, store) = store();
    let tree = Tree::new();
    assert_eq!(read_unknown(&store, &tree).unwrap(), vec![]);
    require_known(&store, &tree).unwrap();
    assert_eq!(read_produced_by(&store, &tree).unwrap(), None);
}

#[test]
fn an_empty_unknown_list_is_no_unknowns() {
    let (_dir, store) = store();
    require_known(&store, &unknown_entry(&store, &[])).unwrap();
}

#[test]
fn a_version_with_unknowns_is_refused_and_says_what_they_are() {
    let (_dir, store) = store();
    let tree = unknown_entry(&store, &[record()]);
    assert_eq!(read_unknown(&store, &tree).unwrap(), vec![record()]);

    let error = require_known(&store, &tree).unwrap_err();
    let refused = error
        .downcast_ref::<VersionHasUnknowns>()
        .expect("a typed error");
    assert_eq!(refused.records, vec![record()]);
    let message = error.to_string();
    assert!(
        message.contains("/schemas/0/units/4/ty") && message.contains("Nothing was changed"),
        "{message}"
    );
}

#[test]
fn an_unknown_entry_that_cannot_be_read_is_an_error_and_not_an_empty_list() {
    let (_dir, store) = store();

    #[derive(Serialize)]
    struct Newer {
        path: String,
        step: String,
        reason: String,
        severity: u8,
    }
    let newer = [Newer {
        path: "/x".into(),
        step: "3-4".into(),
        reason: "r".into(),
        severity: 2,
    }];
    let cases = [
        (
            "garbage",
            tree_with(&store, "unknown", EntryMode::Blob, b"\xff\xff"),
        ),
        ("empty", tree_with(&store, "unknown", EntryMode::Blob, b"")),
        (
            "a field this comline doesn't know",
            tree_with(
                &store,
                "unknown",
                EntryMode::Blob,
                &to_cbor(&newer).unwrap(),
            ),
        ),
        (
            "a tree, not a blob",
            tree_with(
                &store,
                "unknown",
                EntryMode::Tree,
                &to_cbor(&[record()]).unwrap(),
            ),
        ),
    ];
    for (what, tree) in cases {
        let error = require_known(&store, &tree).expect_err(what);
        assert!(
            error.downcast_ref::<VersionHasUnknowns>().is_none(),
            "{what}: {error}"
        );
        assert!(read_unknown(&store, &tree).is_err(), "{what}");
    }
}

#[test]
fn produced_by_is_read_when_there_and_refused_when_it_cannot_be() {
    let (_dir, store) = store();
    let writer = ProducedBy {
        core: "0.1.0".into(),
        spec: "0.11".into(),
    };
    let tree = tree_with(
        &store,
        "produced_by",
        EntryMode::Blob,
        &to_cbor(&writer).unwrap(),
    );
    assert_eq!(read_produced_by(&store, &tree).unwrap(), Some(writer));

    let damaged = tree_with(&store, "produced_by", EntryMode::Blob, b"\xff");
    assert!(read_produced_by(&store, &damaged).is_err());
    let tree = tree_with(&store, "produced_by", EntryMode::Tree, b"");
    assert!(read_produced_by(&store, &tree).is_err());
}

#[test]
fn every_consumer_of_a_version_refuses_one_with_unknowns() {
    use comline_core::package::build::cas::build::compare;

    let (_dir, store) = store();
    let with = unknown_entry(&store, &[record()]);
    let without = Tree::new();

    let is_refused = |error: eyre::Report| error.downcast_ref::<VersionHasUnknowns>().is_some();
    assert!(
        is_refused(doc::read_docs(&store, &with).unwrap_err()),
        "doc"
    );
    assert!(
        is_refused(comline_core::compat::read_history(&store, &with).unwrap_err()),
        "history"
    );
    assert!(
        is_refused(compare(&store, &with, &without).map(|_| ()).unwrap_err()),
        "compare, old"
    );
    assert!(
        is_refused(compare(&store, &without, &with).map(|_| ()).unwrap_err()),
        "compare, new"
    );

    // And the same readers take a version without any.
    doc::read_docs(&store, &without).unwrap();
    assert!(comline_core::compat::read_history(&store, &without)
        .unwrap()
        .is_none());
}
