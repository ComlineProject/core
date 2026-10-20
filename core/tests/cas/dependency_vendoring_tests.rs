// A dependency's frozen schema is vendored into the consumer's own CAS
// commit (a `dep_<name>` subtree), and a real content change in it bumps the
// consumer's version by the change's actual severity — not the blanket
// "Major" `analyze_config_changes` falls back to when it can't see schema
// content (core#6).

use std::fs;
use std::path::Path;

use comline_core::package::build::build;
use comline_core::package::build::cas::objects::{Commit, EntryMode, Tree};
use comline_core::package::build::cas::{refs, ObjectStore};
use comline_core::package::build::VersionBump;
use tempfile::TempDir;

fn write_dependency(dir: &Path, schema: &str) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("config.idp"),
        "congregation shared_types\nspecification_version = 1\n",
    )
    .unwrap();
    fs::write(dir.join("src/models.ids"), schema).unwrap();
}

fn write_consumer(dir: &Path, schema: &str) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("config.idp"),
        "congregation consumer\n\
         specification_version = 1\n\
         \n\
         dependencies = {\n    \
             shared_types = {\n        \
                 path = \"../shared-types\"\n    \
             }\n\
         }\n",
    )
    .unwrap();
    fs::write(dir.join("src/api.ids"), schema).unwrap();
}

fn root_tree(root: &Path) -> (ObjectStore, Tree) {
    let store = ObjectStore::new(root);
    let head = refs::read_ref(root, refs::main_ref()).unwrap();
    let commit = Commit::from_bytes(&store.read(&head).unwrap()).unwrap();
    let tree = Tree::from_bytes(&store.read(&commit.tree).unwrap()).unwrap();
    (store, tree)
}

const CONSUMER_SCHEMA: &str = "use shared_types::models::Thing\n\
    \n\
    struct Holder {\n    \
        thing: Thing\n\
    }\n";

#[test]
fn a_dependency_is_vendored_as_its_own_subtree() {
    let workspace = TempDir::new().unwrap();
    write_dependency(
        &workspace.path().join("shared-types"),
        "struct Thing {\n    id: u64\n}\n",
    );
    let consumer_dir = workspace.path().join("consumer");
    write_consumer(&consumer_dir, CONSUMER_SCHEMA);

    build(&consumer_dir).expect("build with a resolvable dependency");

    let (_store, tree) = root_tree(&consumer_dir);
    let dep_entry = tree
        .entries
        .iter()
        .find(|e| e.name == "dep_shared_types")
        .expect("root tree has a dep_shared_types entry");
    assert_eq!(dep_entry.mode, EntryMode::Tree);
}

#[test]
fn a_breaking_change_in_the_dependency_bumps_the_consumer_major() {
    let workspace = TempDir::new().unwrap();
    let dep_dir = workspace.path().join("shared-types");
    write_dependency(&dep_dir, "struct Thing {\n    id: u64\n}\n");
    let consumer_dir = workspace.path().join("consumer");
    write_consumer(&consumer_dir, CONSUMER_SCHEMA);

    let v1 = build(&consumer_dir).unwrap().current_version;
    assert_eq!(v1, "0.1.0-dev.1");
    comline_core::package::build::release(&consumer_dir, &Default::default()).unwrap();

    // Remove `id` from the dependency's struct — breaking.
    write_dependency(&dep_dir, "struct Thing {\n    name: string\n}\n");
    let res = build(&consumer_dir).unwrap();

    assert_eq!(
        res.version_bump,
        VersionBump::Major,
        "a real breaking change in the dependency's schema, diffed for real, \
         not the config-level blanket Major"
    );
    assert_eq!(res.current_version, "0.2.0-dev.1");
}

#[test]
fn an_additive_change_in_the_dependency_bumps_the_consumer_minor_not_major() {
    let workspace = TempDir::new().unwrap();
    let dep_dir = workspace.path().join("shared-types");
    write_dependency(&dep_dir, "struct Thing {\n    id: u64\n}\n");
    let consumer_dir = workspace.path().join("consumer");
    write_consumer(&consumer_dir, CONSUMER_SCHEMA);

    build(&consumer_dir).unwrap();
    comline_core::package::build::release(&consumer_dir, &Default::default()).unwrap();

    // Add an optional field — additive, not breaking. This is the exact case
    // the old blanket "any dependency change = Major" got wrong.
    write_dependency(
        &dep_dir,
        "struct Thing {\n    id: u64\n    optional nickname: string\n}\n",
    );
    let res = build(&consumer_dir).unwrap();

    assert_eq!(
        res.version_bump,
        VersionBump::Minor,
        "an additive dependency change should bump minor, not the old blanket major"
    );
    assert_eq!(res.current_version, "0.1.1-dev.1");
}

#[test]
fn an_unchanged_dependency_contributes_no_bump() {
    let workspace = TempDir::new().unwrap();
    let dep_dir = workspace.path().join("shared-types");
    write_dependency(&dep_dir, "struct Thing {\n    id: u64\n}\n");
    let consumer_dir = workspace.path().join("consumer");
    write_consumer(&consumer_dir, CONSUMER_SCHEMA);

    build(&consumer_dir).unwrap();

    // Touch only the consumer's own schema (a docstring-only, patch-level
    // change) — the dependency itself hasn't moved.
    write_consumer(
        &consumer_dir,
        &format!("// just a comment\n{CONSUMER_SCHEMA}"),
    );
    let res = build(&consumer_dir).unwrap();

    // No structural change anywhere (a comment isn't a frozen unit), so nothing
    // new to commit at all.
    assert_eq!(res.version_bump, VersionBump::None);
}

#[test]
fn a_dependency_whose_schemas_import_each_other_still_builds() {
    let workspace = TempDir::new().unwrap();
    let dependency = workspace.path().join("shared-types");
    write_dependency(&dependency, "use common::Base\n\nstruct Thing {\n    base: Base\n}\n");
    fs::write(dependency.join("src/common.ids"), "struct Base {\n    id: u64\n}\n").unwrap();
    let consumer_dir = workspace.path().join("consumer");
    write_consumer(&consumer_dir, CONSUMER_SCHEMA);

    build(&consumer_dir).expect("the dependency's own imports resolve against the dependency");
}

#[test]
fn an_import_the_dependency_does_not_declare_fails_the_build() {
    let workspace = TempDir::new().unwrap();
    write_dependency(&workspace.path().join("shared-types"), "struct Thing {\n    id: u64\n}\n");
    let consumer_dir = workspace.path().join("consumer");
    write_consumer(&consumer_dir, "use shared_types::models::Thign\n\nstruct Holder {\n    id: u64\n}\n");

    let error = build(&consumer_dir).expect_err("an unresolved import should fail the build").to_string();
    assert!(error.contains("schema 'shared_types::models' doesn't declare 'Thign'"), "{error}");
    assert!(error.contains("did you mean 'Thing'?"), "{error}");
}

#[test]
fn a_typo_in_the_dependency_name_fails_the_build() {
    let workspace = TempDir::new().unwrap();
    write_dependency(&workspace.path().join("shared-types"), "struct Thing {\n    id: u64\n}\n");
    let consumer_dir = workspace.path().join("consumer");
    write_consumer(&consumer_dir, "use shared_typse::models::Thing\n\nstruct Holder {\n    id: u64\n}\n");

    let error = build(&consumer_dir).expect_err("an unresolved import should fail the build").to_string();
    assert!(error.contains("did you mean 'shared_types'?"), "{error}");
}


/// Every schema in `context` with its frozen units, by namespace.
fn frozen_by_namespace(
    context: &comline_core::package::config::ir::context::ProjectContext,
) -> Vec<(String, Vec<comline_core::schema::ir::frozen::unit::FrozenUnit>)> {
    context
        .schema_contexts
        .iter()
        .map(|schema| {
            let schema = schema.borrow();
            let frozen = schema.frozen_schema.borrow().clone().unwrap();
            (schema.namespace_joined(), frozen)
        })
        .collect()
}

#[test]
fn a_dependency_is_taken_as_its_own_compile_froze_it() {
    use comline_core::package::build::compile_package;
    use comline_core::schema::ir::frozen::unit::FrozenUnit;

    let workspace = TempDir::new().unwrap();
    let dependency = workspace.path().join("shared-types");
    write_dependency(&dependency, "struct Thing {\n    id: u64\n}\n");
    fs::write(
        dependency.join("src/api.ids"),
        "use models::Thing\n\nprotocol Things {\n    function get(id: u64) -> Thing;\n}\n",
    )
    .unwrap();
    let consumer = workspace.path().join("consumer");
    write_consumer(&consumer, CONSUMER_SCHEMA);

    let alone = frozen_by_namespace(&compile_package(&dependency).unwrap());
    let in_consumer = frozen_by_namespace(&compile_package(&consumer).unwrap());

    assert_eq!(alone.len(), 2);
    for (namespace, units) in alone {
        let prefixed = format!("shared_types::{namespace}");
        let expected: Vec<FrozenUnit> = units
            .into_iter()
            .map(|unit| match unit {
                FrozenUnit::Namespace(_) => FrozenUnit::Namespace(prefixed.clone()),
                other => other,
            })
            .collect();
        let found = in_consumer
            .iter()
            .find(|(n, _)| *n == prefixed)
            .unwrap_or_else(|| panic!("no `{prefixed}` in the consumer"));
        assert_eq!(found.1, expected, "`{prefixed}`");
    }
}

#[test]
fn a_dependency_keeps_its_own_settings_and_default_handshake() {
    use comline_core::package::build::compile_package;
    use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};

    let workspace = TempDir::new().unwrap();
    let dependency = workspace.path().join("shared-types");
    write_dependency(&dependency, "struct Thing {\n    id: u64\n}\n");
    // `@unverified` is off by default; this package allows it, and its
    // protocols run no handshake unless they say otherwise.
    fs::write(
        dependency.join("config.idp"),
        "congregation shared_types\nspecification_version = 1\nhandshake = \"none\"\n\
         settings = { annotations = { unverified = { allowed = true } } }\n",
    )
    .unwrap();
    fs::write(
        dependency.join("src/api.ids"),
        "@unverified = \"the 2009 gateway\"\nprotocol Legacy {\n    function ping();\n}\n",
    )
    .unwrap();
    let consumer = workspace.path().join("consumer");
    write_consumer(&consumer, CONSUMER_SCHEMA);
    // The consumer allows neither, and accepts what `Legacy` gives up.
    let manifest = fs::read_to_string(consumer.join("config.idp")).unwrap().replace(
        "path = \"../shared-types\"",
        "path = \"../shared-types\"\n        accept_unverified = { Legacy = [\"schema_agreement\", \"encoding_agreement\"] }",
    );
    fs::write(consumer.join("config.idp"), manifest).unwrap();

    let context = compile_package(&consumer).unwrap();
    let frozen = frozen_by_namespace(&context);
    let (_, api) = frozen.iter().find(|(n, _)| n == "shared_types::api").unwrap();
    let parameters = api
        .iter()
        .find_map(|unit| match unit {
            FrozenUnit::Protocol { name, parameters, .. } if name == "Legacy" => Some(parameters),
            _ => None,
        })
        .unwrap();
    assert!(parameters.contains(&FrozenUnit::Property {
        name: "handshake".to_string(),
        value: ConfigValue::Str("none".to_string()),
    }));
}
