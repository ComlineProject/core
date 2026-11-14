// `live == frozen(current)` (core#268, step 1.2).
//
// The types core persists under `.comline/` and the frozen copy of the
// current format (`comline-ir-formats`) have to encode to the same bytes and
// decode each other's. Three ways of looking: every object of the stores the
// fixture packages build (what core really writes, with a release and a
// second build so a commit has a parent and a release record), values of the
// frozen types no build produces (what only a frozen copy that is wrong
// about a field shows), and the names the trees use.
//
// A change to a live type without the same change in the frozen copy fails
// here. So does a root-tree entry the frozen copy doesn't know: it is a
// ledger row.

use std::fmt::Debug;
use std::fs;

use arbitrary::{Arbitrary, Unstructured};
use comline_core::compat::{self, History};
use comline_core::doc::{self, FrozenDocs};
use comline_core::package::build::cas::object_store::CURRENT_IR_FORMAT_VERSION;
use comline_core::package::build::cas::objects::{Blob, Commit, EntryMode, Tree};
use comline_core::package::build::cas::storage::{from_cbor, to_cbor};
use comline_core::package::build::cas::ObjectStore;
use comline_core::package::build::{build, release, ReleaseRequest};
use comline_core::package::config::ir::frozen::cas::blob as config_blob;
use comline_core::package::config::ir::frozen::FrozenUnit as LiveConfigUnit;
use comline_core::schema::ir::frozen::unit::FrozenUnit as LiveUnit;
use comline_ir_formats::v3::codec;
use comline_ir_formats::v3::entry;
use comline_ir_formats::v3::types as frozen;
use proptest::prelude::*;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::corpus::{self, Built};

/// `bytes` decode as a `L` (the live type) and as a `F` (the frozen copy),
/// and each encodes back to `bytes`.
fn same<L, F>(what: &str, bytes: &[u8])
where
    L: Serialize + DeserializeOwned,
    F: Serialize + DeserializeOwned,
{
    let live: L =
        from_cbor(bytes).unwrap_or_else(|e| panic!("{what}: the live type can't decode it: {e}"));
    let copy: F = codec::decode("value", bytes)
        .unwrap_or_else(|e| panic!("{what}: the frozen copy can't decode what core wrote: {e}"));
    assert_eq!(
        to_cbor(&live).unwrap(),
        bytes,
        "{what}: the live type encodes it differently"
    );
    assert_eq!(
        codec::encode(&copy).unwrap(),
        bytes,
        "{what}: the frozen copy encodes it differently"
    );
}

fn blob_content(store: &ObjectStore, hash: &comline_core::package::build::cas::Hash) -> Vec<u8> {
    let bytes = store.read(hash).unwrap();
    same::<Blob, frozen::Blob>("a blob", &bytes);
    Blob::from_bytes(&bytes).unwrap().content
}

fn schema_tree(store: &ObjectStore, tree: &Tree, whose: &str) {
    for entry in &tree.entries {
        assert_eq!(
            entry.mode,
            EntryMode::Blob,
            "{whose}: a schema tree holds blobs"
        );
        let content = blob_content(store, &entry.hash);
        same::<LiveUnit, frozen::SchemaUnit>(&format!("{whose}/{}", entry.name), &content);
    }
}

fn tree_of(store: &ObjectStore, hash: &comline_core::package::build::cas::Hash) -> Tree {
    let bytes = store.read(hash).unwrap();
    same::<Tree, frozen::Tree>("a tree", &bytes);
    Tree::from_bytes(&bytes).unwrap()
}

fn dependency_tree(store: &ObjectStore, tree: &Tree, whose: &str) {
    for entry in &tree.entries {
        let whose = format!("{whose}/{}", entry.name);
        match (entry.mode, entry.name.as_str()) {
            (EntryMode::Tree, name) if name.starts_with(entry::SCHEMA_PREFIX) => {
                schema_tree(store, &tree_of(store, &entry.hash), &whose)
            }
            (EntryMode::Blob, entry::MANIFEST) => {
                let text = blob_content(store, &entry.hash);
                assert!(
                    String::from_utf8(text)
                        .is_ok_and(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_hexdigit())),
                    "{whose}: the manifest is the content hash in hex"
                );
            }
            (EntryMode::Blob, entry::DEPENDENCY_CONFIG) => {
                same::<Vec<LiveConfigUnit>, Vec<frozen::ConfigUnit>>(&whose, &blob_content(store, &entry.hash))
            }
            (EntryMode::Blob, entry::DEPENDENCY_CONFIGS) => {
                same::<Vec<(String, Vec<LiveConfigUnit>)>, Vec<(String, Vec<frozen::ConfigUnit>)>>(
                    &whose,
                    &blob_content(store, &entry.hash),
                )
            }
            _ => panic!(
                "{whose}: a dependency tree entry the frozen copy of format {CURRENT_IR_FORMAT_VERSION} doesn't know. \
                 Adding one is a ledger row, with the lines in core/formats/src/v{CURRENT_IR_FORMAT_VERSION}"
            ),
        }
    }
}

fn root_tree(store: &ObjectStore, tree: &Tree, whose: &str) {
    for entry in &tree.entries {
        let whose = format!("{whose}/{}", entry.name);
        let name = entry.name.as_str();
        match (entry.mode, name) {
            (EntryMode::Tree, name) if name.starts_with(entry::SCHEMA_PREFIX) => {
                schema_tree(store, &tree_of(store, &entry.hash), &whose)
            }
            (EntryMode::Tree, name) if name.starts_with(entry::DEPENDENCY_PREFIX) => {
                dependency_tree(store, &tree_of(store, &entry.hash), &whose)
            }
            (EntryMode::Blob, entry::CONFIG) => {
                same::<Vec<LiveConfigUnit>, Vec<frozen::ConfigUnit>>(&whose, &blob_content(store, &entry.hash))
            }
            (EntryMode::Blob, entry::DOCS) => {
                same::<FrozenDocs, frozen::FrozenDocs>(&whose, &blob_content(store, &entry.hash))
            }
            (EntryMode::Blob, entry::FACTS) => {
                same::<History, frozen::History>(&whose, &blob_content(store, &entry.hash))
            }
            (EntryMode::Blob, entry::UNKNOWN | entry::PRODUCED_BY) => {
                panic!("{whose}: nothing writes this entry yet, and a fresh build has it")
            }
            _ => panic!(
                "{whose}: a root-tree entry the frozen copy of format {CURRENT_IR_FORMAT_VERSION} doesn't know. \
                 Adding one is a ledger row, with the lines in core/formats/src/v{CURRENT_IR_FORMAT_VERSION}"
            ),
        }
    }
}

/// Every commit from the head back, each root tree under it.
fn history(built: &Built, whose: &str) {
    let mut next = Some(
        comline_core::package::build::cas::read_ref(
            &built.package,
            comline_core::package::build::cas::REF_HEAD_MAIN_FILENAME,
        )
        .unwrap(),
    );
    let mut commits = 0;
    while let Some(hash) = next {
        let bytes = built.store.read(&hash).unwrap();
        same::<Commit, frozen::Commit>(&format!("{whose}: a commit"), &bytes);
        let commit = Commit::from_bytes(&bytes).unwrap();
        let tree = tree_of(&built.store, &commit.tree);
        root_tree(&built.store, &tree, whose);
        next = commit.parents.first().copied();
        commits += 1;
    }
    assert!(
        commits >= 2,
        "{whose}: the history has a release and a build after it"
    );
}

#[test]
fn every_object_core_writes_for_the_fixtures_is_the_frozen_formats() {
    for fixture in corpus::available() {
        let built = Built::of(fixture);
        // A release record, then a build with a parent.
        release(&built.package, &ReleaseRequest::default())
            .unwrap_or_else(|e| panic!("{} releases: {e:?}", fixture.name));
        fs::write(
            built.package.join("src/zz_added.ids"),
            "struct Added {\n    a: u32\n}\n",
        )
        .unwrap();
        build(&built.package).unwrap_or_else(|e| panic!("{} builds again: {e:?}", fixture.name));
        history(&built, fixture.name);
    }
}

#[test]
fn the_frozen_copy_is_the_current_format_and_names_what_core_names() {
    assert_eq!(comline_ir_formats::CURRENT, CURRENT_IR_FORMAT_VERSION);
    assert_eq!(entry::CONFIG, config_blob::CONFIG_ENTRY);
    assert_eq!(entry::DOCS, doc::DOCS_ENTRY);
    assert_eq!(entry::FACTS, compat::FACTS_ENTRY);
    assert_eq!(entry::DEPENDENCY_PREFIX, config_blob::DEPENDENCY_PREFIX);
    assert_eq!(
        entry::DEPENDENCY_CONFIG,
        config_blob::DEPENDENCY_CONFIG_ENTRY
    );
    assert_eq!(
        entry::DEPENDENCY_CONFIGS,
        config_blob::DEPENDENCY_CONFIGS_ENTRY
    );
}

/// A value of the frozen type `F` drawn from `bytes`: what the frozen copy
/// encodes, the live type `L` decodes and encodes back to the same bytes, and
/// the frozen copy decodes to the value. Values no build produces, so a
/// frozen type that is wrong about a field (a name, a default, a skipped
/// field) shows up where no fixture has it.
fn random<F, L>(what: &str, bytes: &[u8])
where
    F: for<'a> Arbitrary<'a> + Serialize + DeserializeOwned + PartialEq + Debug,
    L: Serialize + DeserializeOwned,
{
    let Ok(value) = F::arbitrary(&mut Unstructured::new(bytes)) else {
        return;
    };
    let encoded = codec::encode(&value).unwrap();
    let back: F = match codec::decode("value", &encoded) {
        Ok(back) => back,
        // Nested deeper than the CBOR reader goes: nothing to compare.
        Err(e) if e.to_string().to_lowercase().contains("recursion") => return,
        Err(e) => panic!("{what}: the frozen copy can't decode its own encoding of {value:?}: {e}"),
    };
    assert_eq!(
        back, value,
        "{what}: the frozen copy changed a value through its own bytes"
    );
    let live: L = from_cbor(&encoded).unwrap_or_else(|e| {
        panic!("{what}: the live type can't decode what the frozen copy encodes of {value:?}: {e}")
    });
    assert_eq!(
        to_cbor(&live).unwrap(),
        encoded,
        "{what}: the live type encodes {value:?} differently"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn random_schema_units_are_the_same_bytes(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        random::<frozen::SchemaUnit, LiveUnit>("schema unit", &bytes);
    }

    #[test]
    fn random_config_units_are_the_same_bytes(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        random::<frozen::ConfigUnit, LiveConfigUnit>("config unit", &bytes);
    }

    #[test]
    fn random_docs_and_facts_are_the_same_bytes(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        random::<frozen::FrozenDocs, FrozenDocs>("docs", &bytes);
        random::<frozen::History, History>("facts", &bytes);
    }

    #[test]
    fn random_objects_are_the_same_bytes(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        random::<frozen::Commit, Commit>("commit", &bytes);
        random::<frozen::Tree, Tree>("tree", &bytes);
        random::<frozen::Blob, Blob>("blob", &bytes);
    }
}
