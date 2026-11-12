// What a package's compatibility check sees of its dependencies' configs
// (core#206, core#251): the encodings a dependency declares, and the `wire`
// it sets, are read by `inspect`, so `compat`, `diff` and a stored version
// have to read them too, or they judge a change with less than the full
// picture. A stored version keeps each dependency's frozen config in its
// `dep_<name>` tree; one that doesn't reads as unknown, never as defaults.

use std::fs;
use std::path::Path;

use comline_core::compat::{self, EncodingFacts, PackageFacts, PROFILE_PARAM};
use comline_core::doc::{changelog, frozen_config, frozen_schemas};
use comline_core::inspect;
use comline_core::package::build::cas::build::{compare, working_tree, UnseenConfig};
use comline_core::package::build::cas::objects::{Commit, EntryMode, Tree};
use comline_core::package::build::cas::{main_ref, read_ref, update_ref, ObjectStore};
use comline_core::package::build::{build, compile_package, VersionBump};
use comline_core::package::config::ir::frozen::cas::blob::{self, StoredConfigs};
use comline_core::schema::ir::diff::{BreakingChange, NewFeature, WireFacts};
use comline_core::schema::ir::unverified::Encodings;
use tempfile::TempDir;

/// A framing whose payload is JSON, so a package on it with no `wire.format`
/// gets `json@1`.
fn lines_profile(errors: &str) -> String {
    format!(
        r#"{{
  "name": "com.acme.lines@1",
  "kind": "framing",
  "facts": {{
    "calls": {{ "value": "by_name", "proven_by": ["lines/request"] }},
    "directions": {{ "value": "both", "proven_by": ["lines/request"] }},
    "errors": {{ "value": "{errors}", "proven_by": ["lines/error"] }},
    "payload": {{ "value": "json", "proven_by": ["lines/request"] }}
  }}
}}"#
    )
}

const CBOR_PROFILE: &str = r#"{
  "name": "com.acme.cbor@1",
  "kind": "format",
  "facts": {
    "struct_fields": { "value": "by_name", "proven_by": [] },
    "unknown_field": { "value": "ignored", "proven_by": [] },
    "missing_optional": { "value": "absent", "proven_by": [] },
    "enum_variants": { "value": "by_name", "proven_by": [] },
    "unknown_variant": { "value": "rejected", "proven_by": [] },
    "integer_width": { "value": "by_value", "proven_by": [] },
    "output": { "value": "bytes", "proven_by": [] }
  }
}"#;

const SETTINGS: &str =
    "settings = { annotations = { framing = { allowed = true } unverified = { allowed = true } } }\n";

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// `acme`, which declares the framing (and vectors for it, so that using it
/// gives nothing up), with `errors` as the framing profile states them, and
/// `extra` after its header (its own dependencies, say).
fn write_acme(root: &Path, errors: &str, extra: &str) {
    let dir = root.join("acme");
    write(
        &dir.join("config.idp"),
        &format!(
            "congregation acme\nspecification_version = 1\n{SETTINGS}{extra}\
             encodings = {{\n    \"com.acme.lines@1\" = {{ kind = \"framing\" profile = \"profiles/lines.json\" }}\n}}\n"
        ),
    );
    write(&dir.join("src/api.ids"), "struct Thing {\n    id: u64\n}\n");
    write(&dir.join("profiles/lines.json"), &lines_profile(errors));
    write(&dir.join("profiles/lines.vectors.json"), "[]");
}

/// `base`, a dependency of `acme`'s, which declares a format.
fn write_base(root: &Path) {
    let dir = root.join("base");
    write(
        &dir.join("config.idp"),
        &format!(
            "congregation base\nspecification_version = 1\n{SETTINGS}\
             encodings = {{\n    \"com.acme.cbor@1\" = {{ kind = \"format\" profile = \"profiles/cbor.json\" }}\n}}\n"
        ),
    );
    write(&dir.join("src/api.ids"), "struct Base {\n    id: u64\n}\n");
    write(&dir.join("profiles/cbor.json"), CBOR_PROFILE);
    write(&dir.join("profiles/cbor.vectors.json"), "[]");
}

const BEFORE: &str = "struct S {\n    a: u32\n}\n\nprotocol P {\n    function get() -> S;\n}\n";
const AFTER: &str =
    "struct S {\n    a: u32\n    optional b: u32\n}\n\nprotocol P {\n    function get() -> S;\n}\n";

/// A package in `root/<name>` on `acme`'s framing, with no `wire.format`.
fn write_consumer(root: &Path, name: &str, schema: &str) -> std::path::PathBuf {
    let dir = root.join(name);
    write(
        &dir.join("config.idp"),
        &format!(
            "congregation {name}\nspecification_version = 1\n{SETTINGS}\
             dependencies = {{\n    acme = {{ path = \"../acme\" }}\n}}\n\
             wire = {{ framing = \"com.acme.lines@1\" }}\n"
        ),
    );
    write(&dir.join("src/api.ids"), schema);
    dir
}

fn encoding<'a>(facts: &'a PackageFacts, kind: &str) -> &'a EncodingFacts {
    let found: Vec<_> = facts.encodings.iter().filter(|e| e.kind == kind).collect();
    assert_eq!(found.len(), 1, "one {kind} in play: {:?}", facts.encodings);
    found[0]
}

fn profile_hash(errors: &str) -> String {
    blake3::hash(lines_profile(errors).as_bytes())
        .to_hex()
        .to_string()
}

// ---------- #206: what compat and diff see ----------

/// The visible case: the package sets no `wire.format`, its framing is a
/// dependency's, and that framing writes JSON. `inspect` says `json@1`;
/// compat's encodings and diff's facts have to as well.
#[test]
fn compat_and_diff_read_the_format_a_dependencys_framing_implies() {
    let tmp = TempDir::new().unwrap();
    write_acme(tmp.path(), "by_name", "");
    let dir = write_consumer(tmp.path(), "app", BEFORE);
    let context = compile_package(&dir).unwrap();

    // `inspect`, the reference.
    let summary = inspect::protocol(&context, "api", "P").expect("the protocol");
    assert_eq!(summary.format.value, "json@1");
    assert_eq!(summary.framing.value, "com.acme.lines@1");

    // compat: the encodings the version records.
    let facts = compat::facts_of_context(&context, "0.1.0");
    assert_eq!(encoding(&facts, "format").name, "json@1");
    assert_eq!(encoding(&facts, "framing").name, "com.acme.lines@1");

    // diff: the facts the rules read are json@1's.
    let (schemas, config) = (frozen_schemas(&context), frozen_config(&context));
    let seen = Encodings::of(&context);
    let wire = WireFacts::of((&schemas, &config, &seen), (&schemas, &config, &seen));
    assert!(
        wire.fields_by_name && wire.optional_fields_tolerated && wire.variants_by_name,
        "{wire:?}"
    );
    // What compat and diff saw before: the package's own declarations alone,
    // which don't hold the framing, so core's `packed@1`.
    let partial = Encodings::from_config(&config, [], true);
    let lax = WireFacts::of((&schemas, &config, &partial), (&schemas, &config, &partial));
    assert!(
        !lax.fields_by_name,
        "a view without the dependency reads packed@1"
    );
}

/// The version a build compares uses the full view too: an optional field
/// added under `json@1` is a feature (a minor), under `packed@1` a break.
#[test]
fn a_build_compares_under_the_format_a_dependencys_framing_implies() {
    let tmp = TempDir::new().unwrap();
    write_acme(tmp.path(), "by_name", "");
    let dir = write_consumer(tmp.path(), "app", BEFORE);
    build(&dir).unwrap();

    write(&dir.join("src/api.ids"), AFTER);
    let result = build(&dir).unwrap();
    assert_eq!(
        result.version_bump,
        VersionBump::Minor,
        "{:?}",
        result.schema_changes
    );
    assert!(result.unseen.is_empty(), "{:?}", result.unseen);
    let changes = result.schema_changes.expect("a change");
    assert!(changes.breaking_changes.is_empty(), "{changes:?}");
    assert!(
        changes
            .new_features
            .iter()
            .any(|f| matches!(f, NewFeature::AddedField { field_name, .. } if field_name == "b")),
        "{changes:?}"
    );
}

/// The laxer case: a dependency changes its framing's profile under the same
/// name. The package's encodings now carry that profile, so the parameter
/// hash moves, and a peer on the old one isn't "the same encoding".
#[test]
fn a_dependencys_changed_profile_changes_the_encoding() {
    let tmp = TempDir::new().unwrap();
    write_acme(tmp.path(), "by_name", "");
    let dir = write_consumer(tmp.path(), "app", BEFORE);
    build(&dir).unwrap();
    let before = compat::facts(&dir, None).unwrap();

    let framing = encoding(&before, "framing");
    assert_eq!(framing.name, "com.acme.lines@1");
    assert_eq!(
        framing.params.get(PROFILE_PARAM).map(String::as_str),
        Some(profile_hash("by_name").as_str()),
        "a framing a dependency declares carries its profile"
    );
    assert!(!framing.is_unknown());

    // The dependency states its errors another way; the name stays.
    write_acme(tmp.path(), "by_ordinal", "");
    build(&dir).unwrap();
    let after = compat::facts(&dir, None).unwrap();

    let moved = encoding(&after, "framing");
    assert_eq!(moved.name, framing.name);
    assert_eq!(
        moved.params.get(PROFILE_PARAM),
        Some(&profile_hash("by_ordinal"))
    );
    assert_ne!(
        moved.param_hash, framing.param_hash,
        "the encoding changed under the same name"
    );
    // What didn't change, didn't.
    assert_eq!(encoding(&after, "format"), encoding(&before, "format"));
    assert_eq!(
        encoding(&after, "handshake"),
        encoding(&before, "handshake")
    );
}

/// `changelog` (what `comline diff` and the documentation print) reads the
/// encodings it is given: the full view makes the added optional field a
/// feature, as `compare` does; the package's own config alone would call it
/// a break.
#[test]
fn the_changelog_uses_the_full_view() {
    let tmp = TempDir::new().unwrap();
    write_acme(tmp.path(), "by_name", "");
    let old = compile_package(&write_consumer(tmp.path(), "old", BEFORE)).unwrap();
    let new = compile_package(&write_consumer(tmp.path(), "new", AFTER)).unwrap();

    let full = changelog(
        "0.1.0",
        (
            &frozen_schemas(&old),
            &frozen_config(&old),
            &Encodings::of(&old),
        ),
        "0.2.0",
        (
            &frozen_schemas(&new),
            &frozen_config(&new),
            &Encodings::of(&new),
        ),
    );
    assert!(full.breaking.is_empty(), "{:?}", full.breaking);
    assert!(
        full.features.iter().any(
            |e| matches!(&e.change, NewFeature::AddedField { field_name, .. } if field_name == "b")
        ),
        "{:?}",
        full.features
    );

    // Without the dependencies' declarations (what it read before): `packed@1`.
    let (old_config, new_config) = (frozen_config(&old), frozen_config(&new));
    let (old_seen, new_seen) = (
        Encodings::from_config(&old_config, [], true),
        Encodings::from_config(&new_config, [], true),
    );
    let partial = changelog(
        "0.1.0",
        (&frozen_schemas(&old), &old_config, &old_seen),
        "0.2.0",
        (&frozen_schemas(&new), &new_config, &new_seen),
    );
    assert!(partial
        .breaking
        .iter()
        .any(|e| matches!(e.change, BreakingChange::AddedOptionalField { .. })));
}

// ---------- #251: stored versions keep the configs ----------

fn head(dir: &Path) -> (ObjectStore, Commit, Tree) {
    let store = ObjectStore::new(dir);
    let hash = read_ref(dir, main_ref()).unwrap();
    let commit = Commit::from_bytes(&store.read(&hash).unwrap()).unwrap();
    let tree = Tree::from_bytes(&store.read(&commit.tree).unwrap()).unwrap();
    (store, commit, tree)
}

/// The dependency's `dep_<name>` subtree.
fn dependency_tree(store: &ObjectStore, root: &Tree, name: &str) -> Tree {
    let entry = root
        .find_entry(&format!("dep_{name}"))
        .expect("the dependency is vendored");
    assert_eq!(entry.mode, EntryMode::Tree);
    Tree::from_bytes(&store.read(&entry.hash).unwrap()).unwrap()
}

/// A build stores each dependency's frozen config (and its own
/// dependencies'), `StoredConfigs` reads them back as the compile had them,
/// and the facts a stored version gives are the live package's.
#[test]
fn a_stored_version_gives_the_facts_the_live_package_does() {
    let tmp = TempDir::new().unwrap();
    write_base(tmp.path());
    write_acme(
        tmp.path(),
        "by_name",
        "dependencies = {\n    base = { path = \"../base\" }\n}\n",
    );
    let dir = write_consumer(tmp.path(), "app", BEFORE);
    let context = compile_package(&dir).unwrap();
    assert_eq!(
        context
            .dependency_configs
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>(),
        ["acme", "base"],
        "the dependency, then its own"
    );
    let version = build(&dir).unwrap().current_version;

    let (store, _, tree) = head(&dir);
    let acme = dependency_tree(&store, &tree, "acme");
    let config = acme
        .find_entry(blob::DEPENDENCY_CONFIG_ENTRY)
        .expect("its config is stored");
    assert_eq!(config.mode, EntryMode::Blob);
    assert_eq!(
        blob::read_config(&store, &config.hash).unwrap(),
        context.dependency_configs[0].1
    );
    let theirs = acme
        .find_entry(blob::DEPENDENCY_CONFIGS_ENTRY)
        .expect("so are its dependencies'");
    assert_eq!(
        blob::read_dependency_configs(&store, &theirs.hash).unwrap(),
        context.dependency_configs[1..].to_vec()
    );

    let stored = StoredConfigs::read(&store, &tree).unwrap();
    assert!(stored.complete());
    assert_eq!(Some(&stored.own), context.config_frozen.as_ref());
    assert_eq!(stored.dependencies, context.dependency_configs);
    let (from_tree, live) = (stored.encodings(), Encodings::of(&context));
    assert_eq!(from_tree.declared, live.declared);
    assert_eq!(from_tree.format, live.format);
    assert!(from_tree.complete && live.complete);

    // The round trip: the facts of the stored version are the live package's.
    let live = compat::facts_of_context(&context, &version);
    let stored = compat::facts(&dir, None).unwrap();
    assert_eq!(stored.encodings, live.encodings);
    assert_eq!(stored.encodings.len(), 3);
    assert_eq!(
        stored
            .protocols
            .iter()
            .map(|p| &p.shape_hash)
            .collect::<Vec<_>>(),
        live.protocols
            .iter()
            .map(|p| &p.shape_hash)
            .collect::<Vec<_>>()
    );
    // And what the history recorded is what recomputing it gives.
    let hash = read_ref(&dir, main_ref()).unwrap();
    assert_eq!(
        compat::history_of_commit(&store, &hash).unwrap(),
        compat::compute_history_of_commit(&store, &head(&dir).1, &tree).unwrap()
    );
}

/// A dependency with no config of its own is stored too, as an empty one:
/// what has none is complete, and what predates the blob isn't.
#[test]
fn an_empty_config_is_stored_all_the_same() {
    let tmp = TempDir::new().unwrap();
    let shared = tmp.path().join("shared");
    write(
        &shared.join("config.idp"),
        "congregation shared\nspecification_version = 1\n",
    );
    write(
        &shared.join("src/models.ids"),
        "struct Thing {\n    id: u64\n}\n",
    );
    let dir = tmp.path().join("app");
    write(
        &dir.join("config.idp"),
        "congregation app\nspecification_version = 1\ndependencies = { shared = { path = \"../shared\" } }\n",
    );
    write(
        &dir.join("src/api.ids"),
        "use shared::models::Thing\n\nstruct Holder {\n    thing: Thing\n}\n",
    );
    build(&dir).unwrap();

    let (store, _, tree) = head(&dir);
    let shared = dependency_tree(&store, &tree, "shared");
    assert!(shared.find_entry(blob::DEPENDENCY_CONFIG_ENTRY).is_some());
    assert!(
        shared.find_entry(blob::DEPENDENCY_CONFIGS_ENTRY).is_none(),
        "it has none of its own"
    );
    let stored = StoredConfigs::read(&store, &tree).unwrap();
    assert!(stored.complete(), "{stored:?}");
}

/// Replace the head commit with one whose dependency trees hold no config, as
/// a version stored before core#251 has, and drop the facts it recorded, so
/// they're computed from what's there. The tree before it.
fn store_as_before_configs_were_kept(dir: &Path) -> Tree {
    let (store, commit, tree) = head(dir);
    let mut old = Tree::new();
    for entry in &tree.entries {
        if entry.mode == EntryMode::Tree && entry.name.starts_with("dep_") {
            let kept = Tree::from_bytes(&store.read(&entry.hash).unwrap()).unwrap();
            let mut stripped = Tree::new();
            for e in kept.entries.iter().filter(|e| {
                e.name != blob::DEPENDENCY_CONFIG_ENTRY && e.name != blob::DEPENDENCY_CONFIGS_ENTRY
            }) {
                stripped.add_entry(e.mode, e.name.clone(), e.hash);
            }
            old.add_entry(
                EntryMode::Tree,
                entry.name.clone(),
                store.write(&stripped.to_bytes().unwrap()).unwrap(),
            );
        } else {
            old.add_entry(entry.mode, entry.name.clone(), entry.hash);
        }
    }
    let old = compat::without_facts(&old);
    let mut rewritten = commit;
    rewritten.tree = store.write(&old.to_bytes().unwrap()).unwrap();
    let hash = store.write(&rewritten.to_bytes().unwrap()).unwrap();
    update_ref(dir, main_ref(), &hash).unwrap();
    old
}

/// A tree without the blob is unknown, not defaults that look complete.
#[test]
fn a_tree_stored_without_the_config_reads_as_incomplete() {
    let tmp = TempDir::new().unwrap();
    write_acme(tmp.path(), "by_name", "");
    let dir = write_consumer(tmp.path(), "app", BEFORE);
    build(&dir).unwrap();
    store_as_before_configs_were_kept(&dir);

    let (store, commit, tree) = head(&dir);
    let stored = StoredConfigs::read(&store, &tree).unwrap();
    assert!(stored.own_stored, "its own config was kept");
    assert_eq!(stored.unstored, ["acme"]);
    assert!(!stored.complete());
    assert!(stored.dependencies.is_empty());
    assert!(
        !stored.encodings().complete,
        "a name it can't find reads as unknown"
    );

    // The facts computed from it don't pass the guess off as known: the
    // framing's profile and the format it implies are what a dependency
    // that wasn't kept declares.
    let facts = compat::facts_of_commit(&store, &commit.hash().unwrap()).unwrap();
    for kind in ["framing", "format"] {
        assert!(
            encoding(&facts, kind).is_unknown(),
            "{kind}: {:?}",
            facts.encodings
        );
    }
    assert!(
        !encoding(&facts, "handshake").is_unknown(),
        "an official encoding is known"
    );
    // Complete, the framing carries a profile that makes it a different
    // encoding from any unknown one.
    let live = compat::facts_of_context(&compile_package(&dir).unwrap(), "0.1.0");
    assert_ne!(
        encoding(&facts, "framing").param_hash,
        encoding(&live, "framing").param_hash
    );
}

/// Against a tree that didn't keep the config, compat doesn't claim the
/// package unchanged or the change lenient: it lists what it couldn't see,
/// reads the rules under the strictest facts, and is at least a patch.
#[test]
fn compat_doesnt_claim_what_it_cant_see() {
    let tmp = TempDir::new().unwrap();
    write_acme(tmp.path(), "by_name", "");
    let dir = write_consumer(tmp.path(), "app", BEFORE);
    build(&dir).unwrap();
    let old = store_as_before_configs_were_kept(&dir);

    // The same package, built again with this core: not "no changes".
    let store = ObjectStore::scratch(&dir);
    let context = compile_package(&dir).unwrap();
    let same = working_tree(&dir, &context, &store).unwrap();
    let comparison = compare(&store, &old, &same).unwrap();
    assert_eq!(
        comparison.unseen,
        [UnseenConfig::Dependency("acme".to_string())]
    );
    assert!(
        comparison.bump >= VersionBump::Patch,
        "{:?}",
        comparison.bump
    );

    // An optional field added: json@1 would make it a feature, but what the
    // old version's framing wrote can't be seen, so it's the strictest, a
    // break, as it is whenever the facts aren't known.
    write(&dir.join("src/api.ids"), AFTER);
    let changed = compile_package(&dir).unwrap();
    let new = working_tree(&dir, &changed, &store).unwrap();
    let comparison = compare(&store, &old, &new).unwrap();
    assert_eq!(
        comparison.unseen,
        [UnseenConfig::Dependency("acme".to_string())]
    );
    assert!(
        comparison
            .schema_changes
            .breaking_changes
            .iter()
            .any(|c| matches!(c, BreakingChange::AddedOptionalField { .. })),
        "{:?}",
        comparison.schema_changes
    );
    assert_eq!(comparison.bump, VersionBump::Major);
    // Between two complete trees it is the feature.
    let full = compare(&store, &same, &new).unwrap();
    assert!(full.unseen.is_empty());
    assert_eq!(full.bump, VersionBump::Minor);
}

/// A build on top of such a store says so: its result lists what the
/// comparison couldn't see, the version isn't called unchanged, and the next
/// build, which compares two complete trees, is.
#[test]
fn a_build_over_an_older_store_reports_what_it_couldnt_see() {
    let tmp = TempDir::new().unwrap();
    write_acme(tmp.path(), "by_name", "");
    let dir = write_consumer(tmp.path(), "app", BEFORE);
    build(&dir).unwrap();
    store_as_before_configs_were_kept(&dir);

    let rebuilt = build(&dir).unwrap();
    assert_eq!(
        rebuilt.unseen,
        [UnseenConfig::Dependency("acme".to_string())]
    );
    assert_ne!(rebuilt.version_bump, VersionBump::None);
    assert_ne!(
        rebuilt.previous_version.as_deref(),
        Some(rebuilt.current_version.as_str()),
        "a new version"
    );

    // The store now holds the configs.
    let (store, _, tree) = head(&dir);
    assert!(StoredConfigs::read(&store, &tree).unwrap().complete());
    let again = build(&dir).unwrap();
    assert!(again.unseen.is_empty());
    assert_eq!(again.version_bump, VersionBump::None);
    assert_eq!(again.current_version, rebuilt.current_version);
}
