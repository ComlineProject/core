// A name written in a type is the declaration the schema means, wherever one is
// looked up: `compat` for the shape hashes, the sizer for the byte ranges
// (core#289). Each case names the same declaration a protocol could have
// declared itself, so what a name resolved to shows in a hash and a range that
// the declaration alone would give.

use comline_core::compat;
use comline_core::package::config::ir::context::ProjectContext;

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

#[cfg(feature = "deps")]
#[test]
fn a_dependencys_bare_name_is_not_the_consumers_declaration() {
    assert_eq!(dependency_shape("A", true), dependency_shape("A", false));
}

#[cfg(feature = "deps")]
#[test]
fn a_dependencys_qualified_name_is_not_the_consumers_declaration() {
    assert_eq!(
        dependency_shape("s0::A", true),
        dependency_shape("s0::A", false)
    );
}
