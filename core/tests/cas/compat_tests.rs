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
        format!("congregation test\nid = \"{ID}\"\nspecification_version = 1\nsettings = {{\n    bounds = {{\n        string = {{\n            max = 256\n        }}\n        array = {{\n            max = 1024\n        }}\n    }}\n}}\n{extra}"),
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
fn the_shape_hash_sees_bounds() {
    let shape_of = |source: &str| {
        let dir = package(source);
        build(dir.path()).unwrap();
        shape(&facts(dir.path()), "Users")
    };
    let v = |name: &str, tags: &str| {
        format!(
            "struct User {{\n    id: u64\n    name: {name}\n    tags: {tags}\n}}\n\nprotocol Users {{\n    function get(id: u64) -> User;\n}}\n"
        )
    };
    let base = shape_of(&v("char[max 32]", "u32[max 8]"));

    // A docstring leaves it alone; spelling the same maximum through a
    // constant does too, since it's stored resolved.
    let documented = v("char[max 32]", "u32[max 8]").replace("    name:", "    /// The name.\n    name:");
    assert_eq!(shape_of(&documented), base);
    let constant = format!("const NAME: u32 = 32\n\n{}", v("char[max NAME]", "u32[max 8]"));
    assert_eq!(shape_of(&constant), base);

    // Every maximum travels: a string's, an array's, and the settings
    // default an unwritten one takes (`bounds.string.max = 256`).
    assert_ne!(shape_of(&v("char[max 33]", "u32[max 8]")), base);
    assert_ne!(shape_of(&v("char[max 32]", "u32[max 9]")), base);
    assert_eq!(shape_of(&v("string", "u32[max 8]")), shape_of(&v("char[max 256]", "u32[max 8]")));
    assert_ne!(shape_of(&v("string", "u32[max 8]")), base);
    // A single `char` isn't a string of one.
    assert_ne!(shape_of(&v("char", "u32[max 8]")), shape_of(&v("char[max 1]", "u32[max 8]")));

    // `@max_depth` travels too.
    let tree = |depth: u32| {
        format!(
            "@max_depth = {depth}\nstruct Node {{\n    children: Node[max 4]\n}}\n\nprotocol Users {{\n    function get(id: u64) -> Node;\n}}\n"
        )
    };
    assert_ne!(shape_of(&tree(8)), shape_of(&tree(9)));
    assert_eq!(shape_of(&tree(8).replace("@max_depth", "/// A node.\n@max_depth")), shape_of(&tree(8)));
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

// ---------- wire numbers (core#127) ----------

fn number(facts: &PackageFacts, protocol: &str) -> u16 {
    facts.protocols.iter().find(|p| p.name == protocol).unwrap().number
}

fn fnumber(facts: &PackageFacts, protocol: &str, function: &str) -> u16 {
    let p = facts.protocols.iter().find(|p| p.name == protocol).unwrap();
    p.functions.iter().find(|f| f.name == function).unwrap().number
}

fn history(dir: &Path) -> compat::History {
    let store = ObjectStore::new(dir);
    compat::history_of_commit(&store, &read_ref(dir, main_ref()).unwrap()).unwrap()
}

#[test]
fn numbers_are_assigned_in_order_and_kept_across_builds_and_releases() {
    let dir = package(V1);
    build(dir.path()).unwrap();
    let first = facts(dir.path());
    assert_eq!((number(&first, "Users"), number(&first, "Health")), (0, 1));
    assert_eq!(fnumber(&first, "Users", "get"), 0);
    assert_eq!(fnumber(&first, "Health", "ping"), 0);
    release(dir.path(), &ReleaseRequest::default()).unwrap();

    // A function inserted before the others and a protocol added first:
    // the released ones keep theirs, the new ones come after.
    let more = format!(
        "protocol Admin {{\n    function reset();\n}}\n\n{}",
        V1.replace("    function ping() -> u32;\n", "    function version() -> u32;\n    function ping() -> u32;\n")
    );
    schema(dir.path(), &more);
    build(dir.path()).unwrap();
    let built = facts(dir.path());
    assert_eq!(number(&built, "Admin"), 2);
    assert_eq!((number(&built, "Users"), number(&built, "Health")), (0, 1));
    assert_eq!(fnumber(&built, "Health", "ping"), 0);
    assert_eq!(fnumber(&built, "Health", "version"), 1);
    release(dir.path(), &ReleaseRequest::default()).unwrap();
    let released = facts(dir.path());
    assert_eq!(number(&released, "Admin"), 2);
    assert_eq!(fnumber(&released, "Health", "version"), 1);
    // The earlier release's facts didn't change.
    let old = compat::facts(dir.path(), Some("0.1.0")).unwrap();
    assert_eq!((number(&old, "Users"), number(&old, "Health")), (0, 1));
    assert_reproducible(dir.path());
}

#[test]
fn a_removed_number_is_retired_and_a_re_added_item_gets_a_new_one() {
    let dir = package(V1);
    build(dir.path()).unwrap();
    release(dir.path(), &ReleaseRequest::default()).unwrap();

    // Removed and added back before a release: what builds did and undid
    // doesn't count, it keeps its number.
    schema(dir.path(), &V1.replace("    function ping() -> u32;\n", "    function version() -> u32;\n"));
    build(dir.path()).unwrap();
    assert_eq!(fnumber(&facts(dir.path()), "Health", "version"), 1);
    schema(dir.path(), V1);
    build(dir.path()).unwrap();
    assert_eq!(fnumber(&facts(dir.path()), "Health", "ping"), 0);

    // Removed in a release: retired, under the name that had it.
    let without = V1
        .replace("    function ping() -> u32;\n", "    function version() -> u32;\n")
        .replace("protocol Users {\n    function get(id: u64) -> User;\n}\n\n", "");
    schema(dir.path(), &without);
    build(dir.path()).unwrap();
    release(dir.path(), &ReleaseRequest::default()).unwrap();
    let h = history(dir.path());
    assert_eq!(h.retired, vec![compat::Retired { number: 0, name: "ping::Users".into() }]);
    let health = h.protocols.iter().find(|p| p.path == "ping::Health").unwrap();
    assert_eq!(health.retired, vec![compat::Retired { number: 0, name: "ping".into() }]);
    assert_eq!(health.functions[0].number, 1);

    // Added back: new numbers, above everything the scope ever had.
    schema(dir.path(), &format!("{without}\n{}", "protocol Users {\n    function get(id: u64) -> User;\n}\n")
        .replace("    function version() -> u32;\n", "    function version() -> u32;\n    function ping() -> u32;\n"));
    build(dir.path()).unwrap();
    let back = facts(dir.path());
    assert_eq!(number(&back, "Users"), 2);
    assert_eq!(number(&back, "Health"), 1);
    assert_eq!(fnumber(&back, "Health", "ping"), 2);
    assert_eq!(fnumber(&back, "Health", "version"), 1);
    // Still retired: the numbers are held by new items, the old ones' stay
    // reserved.
    assert!(history(dir.path()).retired.iter().any(|r| r.number == 0));
    assert_reproducible(dir.path());
}

#[test]
fn an_explicit_id_wins_and_assigned_numbers_skip_it() {
    let dir = package(&V1.replace("protocol Health", "@id = 0\nprotocol Health").replace(
        "    function ping() -> u32;\n",
        "    function ping() -> u32;\n    @id = 0\n    function version() -> u32;\n",
    ));
    build(dir.path()).unwrap();
    let f = facts(dir.path());
    assert_eq!((number(&f, "Users"), number(&f, "Health")), (1, 0));
    assert_eq!((fnumber(&f, "Health", "ping"), fnumber(&f, "Health", "version")), (1, 0));
    assert_reproducible(dir.path());

    // Two of the same in one scope.
    schema(dir.path(), &V1.replace("protocol Health", "@id = 3\nprotocol Health").replace("protocol Users", "@id = 3\nprotocol Users"));
    let err = build(dir.path()).unwrap_err().to_string();
    assert!(err.contains("`ping::Users` and `ping::Health` both have `@id = 3`"), "{err}");

    // Out of range.
    schema(dir.path(), &V1.replace("protocol Health", "@id = 70000\nprotocol Health"));
    let err = build(dir.path()).unwrap_err().to_string();
    assert!(err.contains("an id is a u16"), "{err}");
}

#[test]
fn the_reuse_guard_refuses_an_id_history_gave_another_item() {
    let dir = package(V1);
    build(dir.path()).unwrap();
    release(dir.path(), &ReleaseRequest::default()).unwrap();

    // Users holds 0.
    schema(dir.path(), &V1.replace("protocol Health", "@id = 0\nprotocol Health"));
    let err = build(dir.path()).unwrap_err().to_string();
    assert!(err.contains("`@id = 0` on protocol `ping::Health`: 0 was protocol `ping::Users`'s number"), "{err}");

    // `ping` is retired: its number stays reserved.
    schema(dir.path(), &V1.replace("    function ping() -> u32;\n", "    function version() -> u32;\n"));
    build(dir.path()).unwrap();
    release(dir.path(), &ReleaseRequest::default()).unwrap();
    schema(dir.path(), &V1.replace("    function ping() -> u32;\n", "    function version() -> u32;\n    @id = 0\n    function status() -> u32;\n"));
    let err = build(dir.path()).unwrap_err().to_string();
    assert!(err.contains("`@id = 0` on function `status`: 0 was function `ping`'s number"), "{err}");

    // The item that had it may take it back explicitly.
    schema(dir.path(), &V1.replace("    function ping() -> u32;\n", "    function version() -> u32;\n    @id = 0\n    function ping() -> u32;\n"));
    build(dir.path()).unwrap();
    assert_eq!(fnumber(&facts(dir.path()), "Health", "ping"), 0);
    assert!(history(dir.path()).protocols.iter().all(|p| p.retired.is_empty()));
    assert_reproducible(dir.path());
}

#[test]
fn a_released_item_keeps_its_number_when_its_explicit_id_is_dropped() {
    let dir = package(&V1.replace("protocol Health", "@id = 7\nprotocol Health"));
    build(dir.path()).unwrap();
    release(dir.path(), &ReleaseRequest::default()).unwrap();
    assert_eq!(number(&facts(dir.path()), "Health"), 7);
    schema(dir.path(), V1);
    build(dir.path()).unwrap();
    let f = facts(dir.path());
    assert_eq!((number(&f, "Users"), number(&f, "Health")), (0, 7));
    // Changing it is a new number: the old one is retired.
    schema(dir.path(), &V1.replace("protocol Health", "@id = 9\nprotocol Health"));
    build(dir.path()).unwrap();
    assert_eq!(number(&facts(dir.path()), "Health"), 9);
    assert_eq!(history(dir.path()).retired, vec![compat::Retired { number: 7, name: "ping::Health".into() }]);
    assert_reproducible(dir.path());
}

#[test]
fn a_renamed_protocol_keeps_its_number() {
    let dir = package(V1);
    build(dir.path()).unwrap();
    release(dir.path(), &ReleaseRequest::default()).unwrap();
    schema(dir.path(), &V1.replace("protocol Health", "protocol Status"));
    build(dir.path()).unwrap();
    let f = facts(dir.path());
    assert_eq!(number(&f, "Status"), 1);
    assert_eq!(fnumber(&f, "Status", "ping"), 0);
    assert!(history(dir.path()).retired.is_empty());
    assert_reproducible(dir.path());
}

/// A history as stored before numbers: no `number`, `retired` or `numbered`.
#[derive(serde_derive::Serialize)]
struct OldHistory {
    protocols: Vec<OldProtocol>,
}
#[derive(serde_derive::Serialize)]
struct OldProtocol {
    path: String,
    line: String,
    functions: Vec<OldFunction>,
}
#[derive(serde_derive::Serialize)]
struct OldFunction {
    name: String,
    since: String,
}

#[test]
fn a_version_stored_without_numbers_gets_them_computed_when_read() {
    use comline_core::package::build::cas::objects::{Blob, EntryMode};
    use comline_core::package::build::cas::storage::to_cbor;
    use comline_core::package::build::cas::update_ref;

    let dir = package(V1);
    build(dir.path()).unwrap();
    let store = ObjectStore::new(dir.path());
    let hash = read_ref(dir.path(), main_ref()).unwrap();
    let mut commit = Commit::from_bytes(&store.read(&hash).unwrap()).unwrap();
    let tree = Tree::from_bytes(&store.read(&commit.tree).unwrap()).unwrap();
    let stored = compat::read_history(&store, &tree).unwrap().unwrap();
    assert!(stored.numbered);

    // The same commit, its facts as an older core stored them.
    let old = OldHistory {
        protocols: stored
            .protocols
            .iter()
            .map(|p| OldProtocol {
                path: p.path.clone(),
                line: p.line.clone(),
                functions: p.functions.iter().map(|f| OldFunction { name: f.name.clone(), since: f.since.clone() }).collect(),
            })
            .collect(),
    };
    let blob = store.write(&Blob::new(to_cbor(&old).unwrap()).to_bytes().unwrap()).unwrap();
    let mut old_tree = compat::without_facts(&tree);
    old_tree.add_entry(EntryMode::Blob, compat::FACTS_ENTRY.to_string(), blob);
    commit.tree = store.write(&old_tree.to_bytes().unwrap()).unwrap();
    let old_hash = store.write(&commit.to_bytes().unwrap()).unwrap();
    update_ref(dir.path(), main_ref(), &old_hash).unwrap();

    let read = compat::read_history(&store, &old_tree).unwrap().unwrap();
    assert!(!read.numbered);
    assert_eq!(compat::history_of_commit(&store, &old_hash).unwrap(), stored);
    let f = facts(dir.path());
    assert_eq!((number(&f, "Users"), number(&f, "Health")), (0, 1));

    // A build after it numbers against it.
    schema(dir.path(), &format!("{V1}\nprotocol Admin {{\n    function reset();\n}}\n"));
    build(dir.path()).unwrap();
    assert_eq!(number(&facts(dir.path()), "Admin"), 2);
}

// ---------- `comline@2`'s parameters ----------

/// A package with `handshake = <written>`, whose protocol says why where it
/// gives something up.
fn handshake_config(dir: &Path, written: &str) {
    fs::write(
        dir.join("config.idp"),
        format!("congregation test\nid = \"{ID}\"\nspecification_version = 1\nhandshake = {written}\nsettings = {{ annotations = {{ unverified = {{ allowed = true }} }} }}\n"),
    )
    .unwrap();
}

const MARKED: &str = "@unverified = \"rolling out\"\nprotocol P {\n    function f();\n}\n";
const UNMARKED: &str = "protocol P {\n    function f();\n}\n";

fn handshake_facts(written: &str) -> compat::EncodingFacts {
    let dir = package(MARKED);
    handshake_config(dir.path(), written);
    build(dir.path()).unwrap();
    facts(dir.path()).encodings.into_iter().find(|e| e.kind == "handshake").unwrap()
}

#[test]
fn only_a_changed_exchange_moves_the_handshakes_parameter_hash() {
    let plain = handshake_facts("\"comline@2\"");
    assert!(plain.params.is_empty());

    // A default written out hashes as leaving it out.
    for written in [
        "{ name = \"comline@2\" }",
        "{ name = \"comline@2\" exchange = \"offer\" }",
        "{ name = \"comline@2\" scope = \"protocol\" newer_consumer = \"degrade\" same_version_differs = \"refuse\" exchange = \"offer\" }",
    ] {
        assert_eq!(handshake_facts(written), plain, "{written}");
    }
    // A policy isn't in it.
    for written in [
        "{ name = \"comline@2\" scope = \"package\" }",
        "{ name = \"comline@2\" newer_consumer = \"refuse\" }",
        "{ name = \"comline@2\" same_version_differs = \"warn\" }",
    ] {
        assert_eq!(handshake_facts(written).param_hash, plain.param_hash, "{written}");
    }
    // `exchange` is.
    let symmetric = handshake_facts("{ name = \"comline@2\" exchange = \"symmetric\" }");
    assert_ne!(symmetric.param_hash, plain.param_hash);
    assert_eq!(symmetric.params.get("exchange").map(String::as_str), Some("symmetric"));
    assert_eq!(symmetric, compat::encoding("handshake", "comline@2", symmetric.params.clone()));
    // A policy written with it still isn't in it.
    let both = handshake_facts("{ name = \"comline@2\" exchange = \"symmetric\" scope = \"package\" }");
    assert_eq!(both.param_hash, symmetric.param_hash);
}

/// A built version of `from`, released, then `to`: its bump and line.
fn step(from: (&str, &str), to: (&str, &str)) -> (VersionBump, String, PackageFacts, PackageFacts) {
    let dir = package(from.1);
    handshake_config(dir.path(), from.0);
    build(dir.path()).unwrap();
    release(dir.path(), &ReleaseRequest::default()).unwrap();
    let before = facts(dir.path());
    handshake_config(dir.path(), to.0);
    schema(dir.path(), to.1);
    let built = build(dir.path()).unwrap();
    let after = facts(dir.path());
    assert_reproducible(dir.path());
    (built.release_bump, line(&after, "P"), before, after)
}

#[test]
fn changing_exchange_is_a_wire_break_and_moves_the_line() {
    let (bump, line, before, after) =
        step(("\"comline@2\"", UNMARKED), ("{ name = \"comline@2\" exchange = \"symmetric\" }", UNMARKED));
    assert_eq!(bump, VersionBump::Major);
    assert_eq!(line, "0.2.0");
    assert_ne!(shape(&before, "P"), shape(&after, "P"));
}

#[test]
fn a_stricter_or_neutral_policy_is_a_patch_and_moves_no_line() {
    for (from, to) in [
        ("\"comline@2\"", "{ name = \"comline@2\" scope = \"package\" }"),
        ("\"comline@2\"", "{ name = \"comline@2\" newer_consumer = \"refuse\" }"),
        ("{ name = \"comline@2\" newer_consumer = \"refuse\" }", "\"comline@2\""),
    ] {
        let (bump, line, before, after) = step((from, UNMARKED), (to, UNMARKED));
        assert_eq!(bump, VersionBump::Patch, "{from} -> {to}");
        assert_eq!(line, "0.1.0", "{from} -> {to}");
        assert_eq!(shape(&before, "P"), shape(&after, "P"), "{from} -> {to}");
    }
    // Back from `warn`: the guarantee returns, and that's a patch too.
    let warn = "{ name = \"comline@2\" same_version_differs = \"warn\" }";
    let (bump, line, ..) = step((warn, MARKED), ("\"comline@2\"", MARKED));
    assert_eq!(bump, VersionBump::Patch);
    assert_eq!(line, "0.1.0");
}

/// `warn` needs `@unverified` to build at all; marked, it's major, as giving up
/// any guarantee is, but no wire change, so peers on the line stay.
#[test]
fn giving_up_version_honesty_is_major_and_moves_no_line() {
    let warn = "{ name = \"comline@2\" same_version_differs = \"warn\" }";
    let (bump, line, before, after) = step(("\"comline@2\"", MARKED), (warn, MARKED));
    assert_eq!(bump, VersionBump::Major);
    assert_eq!(line, "0.1.0");
    assert_eq!(shape(&before, "P"), shape(&after, "P"));
}

#[test]
fn writing_a_default_out_changes_nothing() {
    let (bump, line, before, after) =
        step(("\"comline@2\"", UNMARKED), ("{ name = \"comline@2\" exchange = \"offer\" scope = \"protocol\" }", UNMARKED));
    assert_eq!(bump, VersionBump::None);
    assert_eq!(line, "0.1.0");
    assert_eq!(shape(&before, "P"), shape(&after, "P"));
}

/// What the step that flips core's defaults reads old versions from: what
/// was in effect is stored, whoever chose it.
#[test]
fn the_stored_history_records_the_effective_encodings() {
    use comline_core::schema::idl::annotations::{DEFAULT_FORMAT, DEFAULT_FRAMING, DEFAULT_HANDSHAKE};

    let stored = |dir: &Path, protocol: &str| {
        let store = ObjectStore::new(dir);
        let commit = Commit::from_bytes(&store.read(&read_ref(dir, main_ref()).unwrap()).unwrap()).unwrap();
        let tree = Tree::from_bytes(&store.read(&commit.tree).unwrap()).unwrap();
        let history = compat::read_history(&store, &tree).unwrap().unwrap();
        let protocol = history.protocols.iter().find(|p| p.path.ends_with(protocol)).unwrap();
        protocol.encodings.iter().map(|e| (e.kind.clone(), e.name.clone())).collect::<Vec<_>>()
    };
    let named = |format: &str, framing: &str, handshake: &str| {
        vec![
            ("format".to_string(), format.to_string()),
            ("framing".to_string(), framing.to_string()),
            ("handshake".to_string(), handshake.to_string()),
        ]
    };

    // The implicit default: nothing in the IR says which, the facts do.
    let dir = package(V1);
    build(dir.path()).unwrap();
    assert_eq!(stored(dir.path(), "Users"), named(DEFAULT_FORMAT, DEFAULT_FRAMING, DEFAULT_HANDSHAKE));

    // And kept at a release, whatever the defaults are later.
    release(dir.path(), &ReleaseRequest::default()).unwrap();
    assert_eq!(stored(dir.path(), "Users"), named(DEFAULT_FORMAT, DEFAULT_FRAMING, DEFAULT_HANDSHAKE));

    // A package's choice and a protocol's own, per protocol.
    let dir = package("@handshake = \"comline@1\"\nprotocol Own {\n    function f();\n}\n\nprotocol P {\n    function f();\n}\n");
    config(dir.path(), "wire = { framing = \"jsonrpc\" }\nhandshake = \"comline@2\"\n");
    build(dir.path()).unwrap();
    assert_eq!(stored(dir.path(), "Own"), named("json@1", "jsonrpc", "comline@1"));
    assert_eq!(stored(dir.path(), "P"), named("json@1", "jsonrpc", "comline@2"));

    // Writing the default framing out changes nothing; any other framing
    // keeps `comline@1`, which needs no connection.
    let dir = package(V1);
    config(dir.path(), "wire = { framing = \"datagram@3\" }\n");
    build(dir.path()).unwrap();
    assert_eq!(stored(dir.path(), "Users"), named(DEFAULT_FORMAT, "datagram@3", DEFAULT_HANDSHAKE));
    let dir = package(V1);
    config(dir.path(), "wire = { framing = \"datagram@2\" }\n");
    build(dir.path()).unwrap();
    assert_eq!(stored(dir.path(), "Users"), named(DEFAULT_FORMAT, "datagram@2", "comline@1"));
}

/// What's read back is what's recorded, not what today's defaults would say.
#[test]
fn the_facts_read_the_recorded_encodings() {
    use comline_core::package::build::cas::objects::{Blob, EntryMode};
    use comline_core::package::build::cas::storage::to_cbor;
    use comline_core::package::build::cas::update_ref;

    let dir = package(V1);
    build(dir.path()).unwrap();
    let store = ObjectStore::new(dir.path());
    let hash = read_ref(dir.path(), main_ref()).unwrap();
    let mut commit = Commit::from_bytes(&store.read(&hash).unwrap()).unwrap();
    let tree = Tree::from_bytes(&store.read(&commit.tree).unwrap()).unwrap();
    let mut history = compat::read_history(&store, &tree).unwrap().unwrap();
    for p in &mut history.protocols {
        for e in &mut p.encodings {
            if e.kind == "handshake" {
                *e = compat::encoding("handshake", "an-older-default", Default::default());
            }
        }
    }
    let blob = store.write(&Blob::new(to_cbor(&history).unwrap()).to_bytes().unwrap()).unwrap();
    let mut changed = compat::without_facts(&tree);
    changed.add_entry(EntryMode::Blob, compat::FACTS_ENTRY.to_string(), blob);
    commit.tree = store.write(&changed.to_bytes().unwrap()).unwrap();
    let new_hash = store.write(&commit.to_bytes().unwrap()).unwrap();
    update_ref(dir.path(), main_ref(), &new_hash).unwrap();

    let read = facts(dir.path());
    assert!(read.encodings.iter().any(|e| e.kind == "handshake" && e.name == "an-older-default"), "{:?}", read.encodings);
}

// ---------- core's defaults moving to `datagram@3` and `comline@2` ----------

const OLD_DEFAULTS: &str = "wire = { framing = \"datagram@2\" }\nhandshake = \"comline@1\"\n";

/// A package built and released before core's defaults moved, taking them:
/// built here stating them, then its head rewritten as an older core stored
/// it, with no `wire` or `handshake` in its config and no encodings in its
/// facts. Its schema IR is the same either way, since neither is frozen in.
fn released_before_the_flip() -> TempDir {
    use comline_core::package::build::cas::objects::{Blob, EntryMode};
    use comline_core::package::build::cas::storage::to_cbor;
    use comline_core::package::build::cas::update_ref;
    use comline_core::package::config::ir::frozen::cas::blob::{read_config, write_config};
    use comline_core::package::config::ir::frozen::FrozenUnit as ConfigUnit;

    let dir = package(V1);
    config(dir.path(), OLD_DEFAULTS);
    build(dir.path()).unwrap();
    release(dir.path(), &ReleaseRequest::default()).unwrap();

    let store = ObjectStore::new(dir.path());
    let hash = read_ref(dir.path(), main_ref()).unwrap();
    let mut commit = Commit::from_bytes(&store.read(&hash).unwrap()).unwrap();
    let tree = Tree::from_bytes(&store.read(&commit.tree).unwrap()).unwrap();
    let mut history = compat::read_history(&store, &tree).unwrap().unwrap();
    for p in &mut history.protocols {
        p.encodings.clear();
    }
    let config_entry = tree.entries.iter().find(|e| e.name == "config").unwrap();
    let stored_config: Vec<ConfigUnit> = read_config(&store, &config_entry.hash)
        .unwrap()
        .into_iter()
        .filter(|u| !matches!(u, ConfigUnit::Wire { .. } | ConfigUnit::Handshake(_)))
        .collect();
    let mut old = compat::without_facts(&tree);
    old.entries.retain(|e| e.name != "config");
    old.add_entry(EntryMode::Blob, "config".to_string(), write_config(&stored_config, &store).unwrap());
    let facts_blob = store.write(&Blob::new(to_cbor(&history).unwrap()).to_bytes().unwrap()).unwrap();
    old.add_entry(EntryMode::Blob, compat::FACTS_ENTRY.to_string(), facts_blob);
    commit.tree = store.write(&old.to_bytes().unwrap()).unwrap();
    update_ref(dir.path(), main_ref(), &store.write(&commit.to_bytes().unwrap()).unwrap()).unwrap();
    // And its config.idp as it was: stating neither.
    config(dir.path(), "");
    dir
}

fn encodings_of(facts: &PackageFacts) -> Vec<(String, String)> {
    facts.encodings.iter().map(|e| (e.kind.clone(), e.name.clone())).collect()
}

/// A version that recorded no encodings reads under the defaults it was
/// built with, and the package's next release takes a breaking bump (a
/// minor one in 0.x), the wire change reported per protocol.
#[test]
fn a_release_before_the_flip_reads_as_built_and_the_next_one_breaks() {
    use comline_core::schema::ir::diff::BreakingChange;

    let dir = released_before_the_flip();
    let old = facts(dir.path());
    assert_eq!(old.version, "0.1.0");
    assert_eq!(
        encodings_of(&old),
        [("format", "packed@1"), ("framing", "datagram@2"), ("handshake", "comline@1")]
            .map(|(k, n)| (k.to_string(), n.to_string()))
    );

    // Rebuilt under today's defaults, as written.
    let built = build(dir.path()).unwrap();
    assert_eq!(built.release_bump, VersionBump::Major);
    let changes = built.schema_changes.expect("the wire changed");
    for (protocol, key) in [("Users", "framing"), ("Users", "handshake"), ("Health", "framing"), ("Health", "handshake")] {
        assert!(
            changes.breaking_changes.iter().any(|c| matches!(c,
                BreakingChange::ChangedWireAnnotation { target, key: k, .. } if target.contains(protocol) && k == key)),
            "{protocol} {key}: {changes:?}"
        );
    }
    let new = facts(dir.path());
    assert_eq!(new.version, "0.2.0-dev.1");
    assert_eq!(
        encodings_of(&new),
        [("format", "packed@1"), ("framing", "datagram@3"), ("handshake", "comline@2")]
            .map(|(k, n)| (k.to_string(), n.to_string()))
    );
    for protocol in ["Users", "Health"] {
        assert_eq!(line(&new, protocol), "0.2.0");
        assert_ne!(shape(&old, protocol), shape(&new, protocol));
        // Numbers were assigned before; they don't move.
        assert_eq!(number(&old, protocol), number(&new, protocol));
    }
    let released = release(dir.path(), &ReleaseRequest::default()).unwrap();
    assert_eq!(released.version, "0.2.0");
}

/// Stating the old defaults keeps the old wire: no wire change, no line
/// moves, the same shape.
#[test]
fn stating_the_old_defaults_keeps_the_wire() {
    let dir = released_before_the_flip();
    let old = facts(dir.path());
    config(dir.path(), OLD_DEFAULTS);
    let built = build(dir.path()).unwrap();
    assert!(built.release_bump <= VersionBump::Patch, "{:?}", built.release_bump);
    assert!(built.schema_changes.is_none(), "{:?}", built.schema_changes);
    let new = facts(dir.path());
    assert_eq!(encodings_of(&new), encodings_of(&old));
    for protocol in ["Users", "Health"] {
        assert_eq!(line(&new, protocol), "0.1.0");
        assert_eq!(shape(&old, protocol), shape(&new, protocol));
    }
}

/// A package stating its encodings sees nothing of the flip: its IR and
/// its facts are what they were.
#[test]
fn a_package_stating_its_encodings_sees_no_change() {
    let dir = package(V1);
    config(dir.path(), OLD_DEFAULTS);
    build(dir.path()).unwrap();
    let f = facts(dir.path());
    assert_eq!(
        encodings_of(&f),
        [("format", "packed@1"), ("framing", "datagram@2"), ("handshake", "comline@1")]
            .map(|(k, n)| (k.to_string(), n.to_string()))
    );
    // Its schema IR has neither frozen in: what a package taking the old
    // defaults had.
    let context = comline_core::package::build::compile_package(dir.path()).unwrap();
    for schema in &context.schema_contexts {
        let schema = schema.borrow();
        let frozen = schema.frozen_schema.borrow();
        for unit in frozen.as_ref().unwrap() {
            if let comline_core::schema::ir::frozen::unit::FrozenUnit::Protocol { parameters, .. } = unit {
                assert!(parameters.is_empty(), "{parameters:?}");
            }
        }
    }
}

// ---------- the working tree's facts, for `comline generation` ----------

fn working(dir: &Path) -> PackageFacts {
    let context = comline_core::package::build::compile_package(dir).unwrap();
    compat::facts_of_working_tree(dir, &context).unwrap()
}

/// What files the store holds, to see nothing was written.
fn stored_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut found = vec![];
    let mut stack = vec![dir.join(".comline")];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            if e.path().is_dir() {
                stack.push(e.path());
            } else {
                found.push(e.path());
            }
        }
    }
    found.sort();
    found
}

#[test]
fn a_package_never_built_has_a_first_builds_facts() {
    let dir = package(V1);
    let f = working(dir.path());
    assert!(!dir.path().join(".comline").exists(), "nothing written");
    build(dir.path()).unwrap();
    assert_eq!(f, facts(dir.path()));
}

#[test]
fn an_unchanged_tree_has_its_builds_facts() {
    let dir = package(V1);
    build(dir.path()).unwrap();
    release(dir.path(), &ReleaseRequest::default()).unwrap();
    assert_eq!(working(dir.path()), facts(dir.path()));
}

/// Numbered as the next build would number it: against the last release
/// and every number retired, without writing anything.
#[test]
fn a_changed_tree_is_numbered_as_its_next_build_would_be() {
    let dir = package(V1);
    build(dir.path()).unwrap();
    release(dir.path(), &ReleaseRequest::default()).unwrap();
    // Health retired (number 1), then Admin added: it can't get 1.
    schema(dir.path(), "struct User {\n    id: u64\n}\n\nprotocol Users {\n    function get(id: u64) -> User;\n}\n");
    build(dir.path()).unwrap();
    schema(dir.path(), "struct User {\n    id: u64\n}\n\nprotocol Users {\n    function get(id: u64) -> User;\n    function list() -> u32;\n}\n\nprotocol Admin {\n    function reset();\n}\n");

    let head = read_ref(dir.path(), main_ref()).unwrap();
    let before = stored_files(dir.path());
    let f = working(dir.path());
    assert_eq!(stored_files(dir.path()), before, "nothing written");
    assert_eq!(read_ref(dir.path(), main_ref()).unwrap(), head);

    build(dir.path()).unwrap();
    let built = facts(dir.path());
    assert_eq!(f, built);
    assert_eq!(number(&f, "Admin"), 2);
    assert_eq!(fnumber(&f, "Users", "list"), 1);
}
