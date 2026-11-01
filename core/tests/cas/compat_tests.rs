// What each version provides for `comline@2`: the package id, each
// protocol's line and wire-shape hash, each function's "since", and the
// encodings' parameter hashes (`comline_core::compat`).

use std::fs;
use std::path::Path;

use comline_core::compat::{self, PackageFacts};
use comline_core::package::build::cas::objects::{Commit, Tree};
use comline_core::package::build::cas::{main_ref, read_ref, ObjectStore};
use comline_core::package::build::{build, release, ReleaseRequest, VersionBump};
use tempfile::TempDir;

const ID: &str = "0123456789abcdef0123456789abcdef";

fn package(source: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    config(dir.path(), "");
    fs::create_dir(dir.path().join("src")).unwrap();
    schema(dir.path(), source);
    dir
}

fn config(dir: &Path, extra: &str) {
    fs::write(
        dir.join("config.idp"),
        format!("congregation test\nid = \"{ID}\"\nspecification_version = 1\n{extra}"),
    )
    .unwrap();
}

fn schema(dir: &Path, source: &str) {
    fs::write(dir.join("src/ping.ids"), source).unwrap();
}

fn facts(dir: &Path) -> PackageFacts {
    compat::facts(dir, None).unwrap()
}

fn line(facts: &PackageFacts, protocol: &str) -> String {
    facts.protocols.iter().find(|p| p.name == protocol).unwrap().line.clone()
}

fn since(facts: &PackageFacts, protocol: &str, function: &str) -> String {
    let p = facts.protocols.iter().find(|p| p.name == protocol).unwrap();
    p.functions.iter().find(|f| f.name == function).unwrap().since.clone()
}

fn shape(facts: &PackageFacts, protocol: &str) -> String {
    facts.protocols.iter().find(|p| p.name == protocol).unwrap().shape_hash.clone()
}

const V1: &str = "struct User {\n    id: u64\n}\n\nprotocol Users {\n    function get(id: u64) -> User;\n}\n\nprotocol Health {\n    function ping() -> u32;\n}\n";

/// Every commit's stored history is what recomputing it from what came
/// before gives.
fn assert_reproducible(dir: &Path) {
    let store = ObjectStore::new(dir);
    let mut hash = read_ref(dir, main_ref()).unwrap();
    loop {
        let commit = Commit::from_bytes(&store.read(&hash).unwrap()).unwrap();
        let tree = Tree::from_bytes(&store.read(&commit.tree).unwrap()).unwrap();
        let stored = compat::read_history(&store, &tree).unwrap().expect("a stored history");
        let computed = compat::compute_history_of_commit(&store, &commit, &tree).unwrap();
        assert_eq!(stored, computed, "history of {}", commit.version);
        match commit.parents.first() {
            Some(parent) => hash = *parent,
            None => break,
        }
    }
}

#[test]
fn lines_move_only_for_the_protocol_that_breaks() {
    let dir = package(V1);
    build(dir.path()).unwrap();
    let first = facts(dir.path());
    assert_eq!(first.id.as_deref(), Some(ID));
    assert_eq!(first.version, "0.1.0-dev.1");
    assert_eq!(line(&first, "Users"), "0.1.0");
    release(dir.path(), &ReleaseRequest::default()).unwrap();

    // A function added: a minor, no line moves, it's new since 0.1.1.
    schema(dir.path(), &V1.replace("    function ping() -> u32;\n", "    function ping() -> u32;\n    function version() -> u32;\n"));
    build(dir.path()).unwrap();
    let added = facts(dir.path());
    assert_eq!(added.version, "0.1.1-dev.1");
    assert_eq!(line(&added, "Health"), "0.1.0");
    assert_eq!(since(&added, "Health", "ping"), "0.1.0");
    assert_eq!(since(&added, "Health", "version"), "0.1.1");

    // A type `Users` reaches breaks: its line moves to the release this
    // leads to, `Health`'s stays.
    let broken = V1.replace("id: u64\n}", "id: string\n}")
        .replace("    function ping() -> u32;\n", "    function ping() -> u32;\n    function version() -> u32;\n");
    schema(dir.path(), &broken);
    build(dir.path()).unwrap();
    let built = facts(dir.path());
    assert_eq!(built.version, "0.2.0-dev.2");
    assert_eq!(line(&built, "Users"), "0.2.0");
    assert_eq!(line(&built, "Health"), "0.1.0");
    // What a build leads to now, not what an earlier build led to.
    assert_eq!(since(&built, "Health", "version"), "0.2.0");

    let released = release(dir.path(), &ReleaseRequest::default()).unwrap();
    assert_eq!(released.version, "0.2.0");
    let at_release = facts(dir.path());
    assert_eq!(line(&at_release, "Users"), "0.2.0");
    assert_eq!(line(&at_release, "Health"), "0.1.0");
    assert_eq!(compat::facts(dir.path(), Some("0.1.0")).unwrap().protocols.len(), 2);

    assert_reproducible(dir.path());
}

#[test]
fn a_function_removed_and_added_again_starts_again() {
    let dir = package(V1);
    build(dir.path()).unwrap();
    release(dir.path(), &ReleaseRequest::default()).unwrap();

    schema(dir.path(), &V1.replace("    function ping() -> u32;\n", ""));
    build(dir.path()).unwrap();
    assert_eq!(line(&facts(dir.path()), "Health"), "0.2.0");
    release(dir.path(), &ReleaseRequest::default()).unwrap();

    schema(dir.path(), V1);
    build(dir.path()).unwrap();
    let back = facts(dir.path());
    assert_eq!(since(&back, "Health", "ping"), "0.2.1");
    assert_eq!(line(&back, "Users"), "0.1.0");
    assert_reproducible(dir.path());
}

#[test]
fn a_release_as_another_version_renames_what_its_builds_led_to() {
    let dir = package(V1);
    build(dir.path()).unwrap();
    release(dir.path(), &ReleaseRequest::default()).unwrap();

    schema(dir.path(), &V1.replace("    function ping() -> u32;\n", "    function ping() -> u32;\n    function version() -> u32;\n"));
    build(dir.path()).unwrap();
    assert_eq!(since(&facts(dir.path()), "Health", "version"), "0.1.1");

    let raised = ReleaseRequest { raise: Some(VersionBump::Major), reason: Some("behaviour".into()), stable: false };
    assert_eq!(release(dir.path(), &raised).unwrap().version, "0.2.0");
    let released = facts(dir.path());
    assert_eq!(since(&released, "Health", "version"), "0.2.0");
    // A raise isn't a break: the lines stay.
    assert_eq!(line(&released, "Health"), "0.1.0");
    assert_reproducible_except_releases(dir.path());
}

/// A release made as another version than its builds led to stores the
/// renamed history; every build's stored one still recomputes.
fn assert_reproducible_except_releases(dir: &Path) {
    let store = ObjectStore::new(dir);
    let mut hash = read_ref(dir, main_ref()).unwrap();
    loop {
        let commit = Commit::from_bytes(&store.read(&hash).unwrap()).unwrap();
        let tree = Tree::from_bytes(&store.read(&commit.tree).unwrap()).unwrap();
        let stored = compat::read_history(&store, &tree).unwrap().unwrap();
        let computed = compat::compute_history_of_commit(&store, &commit, &tree).unwrap();
        assert_eq!(stored, computed, "history of {}", commit.version);
        match commit.parents.first() {
            Some(parent) => hash = *parent,
            None => break,
        }
    }
}

#[test]
fn a_package_wire_change_moves_every_line() {
    let dir = package(V1);
    build(dir.path()).unwrap();
    release(dir.path(), &ReleaseRequest::default()).unwrap();
    let before = facts(dir.path());

    config(dir.path(), "wire = { format = \"msgpack@1\" }\n");
    build(dir.path()).unwrap();
    let after = facts(dir.path());
    assert_eq!(line(&after, "Users"), "0.2.0");
    assert_eq!(line(&after, "Health"), "0.2.0");
    assert_ne!(shape(&before, "Health"), shape(&after, "Health"));
    assert!(after.encodings.iter().any(|e| e.kind == "format" && e.name == "msgpack@1"));
    assert_reproducible(dir.path());
}

#[test]
fn the_shape_hash_sees_the_wire_and_nothing_else() {
    let shape_of = |source: &str, file: &str| {
        let dir = package("");
        fs::remove_file(dir.path().join("src/ping.ids")).unwrap();
        fs::write(dir.path().join("src").join(file), source).unwrap();
        build(dir.path()).unwrap();
        let f = facts(dir.path());
        (shape(&f, "Users"), shape(&f, "Health"))
    };
    let (base, health) = shape_of(V1, "ping.ids");

    // Docstrings, spans, the namespace and spelling a type through an
    // alias leave it alone.
    let documented = V1.replace("protocol Users {", "/// The users.\nprotocol Users {");
    assert_eq!(shape_of(&documented, "ping.ids").0, base);
    let moved = format!("\n\n\n{V1}");
    assert_eq!(shape_of(&moved, "ping.ids").0, base);
    assert_eq!(shape_of(V1, "pong.ids").0, base);
    let aliased = V1.replace("get(id: u64)", "get(id: UserId)").replace("struct User", "type UserId = u64\n\nstruct User");
    assert_eq!(shape_of(&aliased, "ping.ids").0, base);
    let constant = format!("const LIMIT: u32 = 3\n\n{V1}");
    assert_eq!(shape_of(&constant, "ping.ids").0, base);

    // What travels changes it, and only for the protocol that reaches it.
    let field = V1.replace("id: u64\n}", "id: u64\n    name: string\n}");
    let (users, health_after) = shape_of(&field, "ping.ids");
    assert_ne!(users, base);
    assert_eq!(health_after, health);
    let renamed_function = V1.replace("function get(", "function fetch(");
    assert_ne!(shape_of(&renamed_function, "ping.ids").0, base);
    let argument = V1.replace("get(id: u64)", "get(id: u32)");
    assert_ne!(shape_of(&argument, "ping.ids").0, base);
    let throws = format!("error NotFound {{\n    message = \"no such user\"\n}}\n\n{}", V1.replace("-> User;", "-> User ! NotFound;"));
    assert_ne!(shape_of(&throws, "ping.ids").0, base);
}

#[test]
fn encodings_carry_a_hash_of_their_parameters() {
    use std::collections::BTreeMap;
    let a = compat::encoding("framing", "jsonrpc", BTreeMap::new());
    let b = compat::encoding("framing", "jsonrpc", BTreeMap::from([("method_names".to_string(), "qualified".to_string())]));
    assert_ne!(a.param_hash, b.param_hash);
    assert_eq!(a, compat::encoding("framing", "jsonrpc", BTreeMap::new()));
}

#[test]
fn changing_the_id_is_major_and_a_malformed_one_is_refused() {
    let dir = package(V1);
    build(dir.path()).unwrap();
    fs::write(
        dir.path().join("config.idp"),
        "congregation test\nid = \"ffffffffffffffffffffffffffffffff\"\nspecification_version = 1\n",
    )
    .unwrap();
    assert_eq!(build(dir.path()).unwrap().version_bump, VersionBump::Major);

    fs::write(dir.path().join("config.idp"), "congregation test\nid = \"nope\"\nspecification_version = 1\n").unwrap();
    let err = build(dir.path()).err().expect("a malformed id is refused").to_string();
    assert!(err.contains("hex"), "{err}");
}

#[test]
fn a_version_built_before_facts_gets_them_computed() {
    let dir = package(V1);
    build(dir.path()).unwrap();
    let store = ObjectStore::new(dir.path());
    let hash = read_ref(dir.path(), main_ref()).unwrap();
    let commit = Commit::from_bytes(&store.read(&hash).unwrap()).unwrap();
    let tree = Tree::from_bytes(&store.read(&commit.tree).unwrap()).unwrap();
    let bare = compat::without_facts(&tree);
    assert!(compat::read_history(&store, &bare).unwrap().is_none());
    let computed = compat::compute_history_of_commit(&store, &commit, &bare).unwrap();
    assert_eq!(Some(computed), compat::read_history(&store, &tree).unwrap());
}

/// The `Users` shape hash of `source` under `config.idp` extras `wire`.
fn shape_under(wire: &str, source: &str) -> String {
    let dir = package(source);
    config(dir.path(), wire);
    build(dir.path()).unwrap();
    shape(&facts(dir.path()), "Users")
}

const TWO: &str = "struct User {\n    id: u64\n    name: string\n}\n\nprotocol Users {\n    function get(id: u64) -> User;\n    function count() -> u32;\n}\n";

#[test]
fn function_order_counts_only_where_calls_go_by_index() {
    let reordered = TWO.replace(
        "    function get(id: u64) -> User;\n    function count() -> u32;\n",
        "    function count() -> u32;\n    function get(id: u64) -> User;\n",
    );
    // JSON-RPC calls by name: a reorder is no change on the wire.
    let jsonrpc = "wire = { framing = \"jsonrpc\" }\n";
    assert_eq!(shape_under(jsonrpc, TWO), shape_under(jsonrpc, &reordered));
    // `datagram@2` calls by index: it is.
    assert_ne!(shape_under("", TWO), shape_under("", &reordered));
}

#[test]
fn field_order_counts_only_where_fields_go_by_position() {
    let reordered = TWO.replace("    id: u64\n    name: string\n", "    name: string\n    id: u64\n");
    // `json@1` keys fields by name.
    let json = "wire = { format = \"json@1\" }\n";
    assert_eq!(shape_under(json, TWO), shape_under(json, &reordered));
    // `packed@1` by position.
    assert_ne!(shape_under("", TWO), shape_under("", &reordered));
}

#[test]
fn protocols_of_the_same_name_in_two_schemas_have_their_own_facts() {
    let api = |ty: &str, field: &str| {
        format!("struct {ty} {{\n    v: {field}\n}}\n\nprotocol Api {{\n    function get() -> {ty};\n}}\n")
    };
    let dir = package("");
    fs::remove_file(dir.path().join("src/ping.ids")).unwrap();
    fs::write(dir.path().join("src/alpha.ids"), api("Alpha", "u32")).unwrap();
    fs::write(dir.path().join("src/beta.ids"), api("Beta", "u32")).unwrap();
    build(dir.path()).unwrap();
    release(dir.path(), &ReleaseRequest::default()).unwrap();

    // Only beta's `Api` breaks.
    fs::write(dir.path().join("src/beta.ids"), api("Beta", "string")).unwrap();
    build(dir.path()).unwrap();
    let f = facts(dir.path());
    let apis: Vec<_> = f.protocols.iter().filter(|p| p.name == "Api").collect();
    assert_eq!(apis.len(), 2, "{f:?}");
    let line_of = |ns: &str| apis.iter().find(|p| p.namespace.ends_with(ns)).unwrap().line.clone();
    assert_eq!(line_of("alpha"), "0.1.0");
    assert_eq!(line_of("beta"), "0.2.0");
    assert!(apis.iter().all(|p| p.path == format!("{}::Api", p.namespace)));
    assert_reproducible(dir.path());
}

#[test]
fn a_renamed_namespace_keeps_its_protocols_history() {
    let dir = package(V1);
    build(dir.path()).unwrap();
    release(dir.path(), &ReleaseRequest::default()).unwrap();
    fs::rename(dir.path().join("src/ping.ids"), dir.path().join("src/pong.ids")).unwrap();
    build(dir.path()).unwrap();
    let f = facts(dir.path());
    assert_eq!(f.version, "0.2.0-dev.1", "a rename is still major for code");
    assert!(f.protocols.iter().all(|p| p.namespace.ends_with("pong")), "{f:?}");
    assert_eq!(line(&f, "Users"), "0.1.0");
    assert_eq!(since(&f, "Health", "ping"), "0.1.0");
}
