// A name written in a type is the declaration the schema means, wherever one is
// looked up: `compat` for the shape hashes, the sizer for the byte ranges
// (core#289). Each case names the same declaration a protocol could have
// declared itself, so what a name resolved to shows in a hash and a range that
// the declaration alone would give.

use comline_core::compat;
use comline_core::package::build::PackageSources;
use comline_core::package::config::ir::context::ProjectContext;
use comline_core::schema::ir::frozen::types::TypeRef;
use comline_core::schema::ir::frozen::unit::FrozenUnit;
use comline_core::schema::ir::size::{self, FrozenLookup};

/// The two `User`s a case tells apart: a different size and a different shape.
const BIG: &str = "struct User {\n    id: u64\n}\n";
const SMALL: &str = "struct User {\n    id: u8\n}\n";

fn users(use_line: &str, reference: &str) -> String {
    format!("{use_line}\n\nprotocol Users {{\n    function get() -> {reference};\n}}\n")
}

fn compile(schemas: &[(&[&str], String)]) -> ProjectContext {
    let mut sources = PackageSources::new();
    for (namespace, source) in schemas {
        sources = sources.schema(namespace.iter().copied(), source.clone());
    }
    sources
        .compile()
        .unwrap_or_else(|e| panic!("should compile: {e}"))
}

fn shape(context: &ProjectContext, protocol: &str) -> String {
    let facts = compat::facts_of_context(context, "0.1.0");
    facts
        .protocols
        .iter()
        .find(|p| p.name == protocol)
        .unwrap()
        .shape_hash
        .clone()
}

/// What `reference` takes under `packed@1`, written in the schema `namespace`;
/// `packages` are the names of the dependencies the context has.
fn size_of(
    context: &ProjectContext,
    namespace: &str,
    reference: &str,
    packages: &[&str],
) -> Option<size::Range> {
    let frozen: Vec<Vec<FrozenUnit>> = context
        .schema_contexts
        .iter()
        .filter_map(|s| s.borrow().frozen_schema.borrow().clone())
        .collect();
    let lookup =
        FrozenLookup::with_packages(frozen.iter().map(Vec::as_slice), packages.iter().copied());
    let scope = lookup.scope_of(namespace).expect("the schema");
    size::range(
        &TypeRef::Named(reference.to_string()),
        size::rules("packed@1").unwrap(),
        &lookup,
        scope,
    )
}

/// The shape and the size of a protocol returning a `User` declared by the
/// protocol's own schema: what any way of reaching that `User` has to give.
fn declared_here(user: &str) -> (String, Option<size::Range>) {
    let source = format!("{user}\n{}", users("", "User"));
    let context = compile(&[(&["app"], source)]);
    (
        shape(&context, "Users"),
        size_of(&context, "app", "User", &[]),
    )
}

/// Schemas that hold the same name under namespaces that resemble each other,
/// in both orders.
fn pairs() -> Vec<(&'static str, [&'static [&'static str]; 2])> {
    vec![
        ("v1::api, v2::api", [&["v1", "api"], &["v2", "api"]]),
        ("v2::api, v1::api", [&["v2", "api"], &["v1", "api"]]),
        ("rapi, api", [&["rapi"], &["api"]]),
        ("api, rapi", [&["api"], &["rapi"]]),
        ("v1::api, api", [&["v1", "api"], &["api"]]),
        ("api, v1::api", [&["api"], &["v1", "api"]]),
    ]
}

#[test]
fn a_name_resolves_to_the_declaration_the_schema_imports() {
    let (big, small) = (declared_here(BIG), declared_here(SMALL));
    assert_ne!(big.0, small.0, "the two declarations have different shapes");
    assert_ne!(big.1, small.1, "and different sizes");

    let mut wrong = vec![];
    for (label, pair) in pairs() {
        // The first schema of the pair holds the big `User`, the second the
        // small one: each is imported in turn, the other is what a loose
        // lookup could take instead.
        for imported in 0..2 {
            let (path, expected) = (
                pair[imported].join("::"),
                if imported == 0 { &big } else { &small },
            );
            // What is written, and what it freezes as: a qualified name is
            // frozen as its path from the package's root (core#290), however
            // it was written.
            let forms = [
                (
                    "bare",
                    format!("use package::{path}::User"),
                    "User".to_string(),
                    "User".to_string(),
                ),
                (
                    "aliased",
                    format!("use package::{path}::User as Account"),
                    "Account".to_string(),
                    "Account".to_string(),
                ),
                (
                    "qualified",
                    format!("use package::{path}::User"),
                    format!("package::{path}::User"),
                    format!("{path}::User"),
                ),
                (
                    "qualified by a namespace",
                    format!("use package::{path}"),
                    format!("{}::User", pair[imported].last().unwrap()),
                    format!("{path}::User"),
                ),
            ];
            for (form, use_line, written, reference) in forms {
                let reference = reference.as_str();
                let context = compile(&[
                    (pair[0], BIG.to_string()),
                    (pair[1], SMALL.to_string()),
                    (&["app"], users(&use_line, &written)),
                ]);
                let shape = shape(&context, "Users");
                if shape != expected.0 {
                    wrong.push(format!("compat: {label}, imports {path}, {form}"));
                }
                if size_of(&context, "app", reference, &[]) != expected.1 {
                    wrong.push(format!("sizer: {label}, imports {path}, {form}"));
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "resolved to another declaration than the schema imports:\n{}",
        wrong.join("\n")
    );
}

// ---------- a dependency's names are its own package's ----------

#[cfg(feature = "deps")]
fn write(path: &std::path::Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A dependency `p0` whose schema `s1` holds `field` of its sibling `s0`'s
/// `A`, and a package `p1` using that. `p1`'s own `s0` is a schema of the
/// same name, with an `A` of its own when `decoy`.
#[cfg(feature = "deps")]
fn consumer(field: &str, decoy: bool) -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    write(
        &root.join("p0/config.idp"),
        "congregation p0\nspecification_version = 1\n",
    );
    write(&root.join("p0/src/s0.ids"), "struct A {\n    f0: u8\n}\n");
    write(
        &root.join("p0/src/s1.ids"),
        &format!("use package::s0::A\n\nstruct B {{\n    f0: {field}\n}}\n"),
    );
    write(
        &root.join("p1/config.idp"),
        "congregation p1\nspecification_version = 1\ndependencies = {\n    p0 = {\n        path = \"../p0\"\n    }\n}\n",
    );
    let own = if decoy {
        "struct A {\n    f0: u64\n}\n\n"
    } else {
        ""
    };
    write(
        &root.join("p1/src/s0.ids"),
        &format!("use p0::s1::B\n\n{own}protocol Q {{\n    function f(a0: B);\n}}\n"),
    );
    tmp
}

#[cfg(feature = "deps")]
fn dependency_shape(field: &str, decoy: bool) -> String {
    let tmp = consumer(field, decoy);
    let context =
        comline_core::package::build::compile_package(&tmp.path().join("p1")).expect("p1 builds");
    shape(&context, "Q")
}

/// What `B` takes where `p1` imports it, as the sizer reads it.
#[cfg(feature = "deps")]
fn dependency_size(field: &str, decoy: bool) -> Option<size::Range> {
    let tmp = consumer(field, decoy);
    let context =
        comline_core::package::build::compile_package(&tmp.path().join("p1")).expect("p1 builds");
    size_of(&context, "s0", "B", &["p0", "std"])
}

#[cfg(feature = "deps")]
#[test]
fn a_dependencys_bare_name_is_not_the_consumers_declaration() {
    assert_eq!(dependency_shape("A", true), dependency_shape("A", false));
}

#[cfg(feature = "deps")]
#[test]
fn a_dependencys_qualified_name_is_not_the_consumers_declaration() {
    assert_eq!(
        dependency_shape("package::s0::A", true),
        dependency_shape("package::s0::A", false)
    );
}

#[cfg(feature = "deps")]
#[test]
fn the_sizer_reads_a_dependencys_names_in_its_package() {
    for field in ["A", "package::s0::A"] {
        let alone = dependency_size(field, false);
        assert!(alone.is_some(), "`{field}` is sized");
        assert_eq!(dependency_size(field, true), alone, "`{field}`");
    }
}
