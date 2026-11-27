//! Named connection sets (core#301): what the conformance cases (`connection`
//! group, run by `compiler_cases`) don't say. `@connection` is read from the
//! source and never frozen, every error is collected, and each message says
//! which part of an encoding a set can't have.

use comline_core::package::build::compile_package;
use comline_core::schema::ir::frozen::unit::FrozenUnit;

fn write(dir: &std::path::Path, config: &str, schema: &str) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("config.idp"),
        format!("congregation app\nspecification_version = 1\n{config}"),
    )
    .unwrap();
    std::fs::write(dir.join("src/api.ids"), schema).unwrap();
}

/// What building the package says: its error, or `None` if it built.
fn error_of(config: &str, schema: &str) -> Option<String> {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), config, schema);
    compile_package(tmp.path()).err().map(|e| format!("{e:?}"))
}

#[test]
fn connection_is_not_frozen_and_the_protocols_parameters_are_as_without_it() {
    let tmp = tempfile::tempdir().unwrap();
    let schema = "@connection = admin\nprotocol Ops {\n    function restart();\n}\n\n\
                  @connection = [admin, public]\nprotocol Health {\n    function ping();\n}\n\n\
                  @connection = public\nprotocol Chat {\n    function send();\n}\n";
    write(tmp.path(), "handshake = \"comline@2\"\n", schema);
    let context = compile_package(tmp.path()).unwrap();

    let mut protocols = 0;
    for schema in &context.schema_contexts {
        let schema = schema.borrow();
        let frozen = schema.frozen_schema.borrow();
        for unit in frozen.iter().flatten() {
            let FrozenUnit::Protocol { parameters, .. } = unit else {
                continue;
            };
            protocols += 1;
            // Neither the name (`Property`) nor a list's items (which the
            // generic freezing would make `ValidatorRef`s) are in the IR.
            for parameter in parameters {
                match parameter {
                    FrozenUnit::Property { name, .. } => assert_ne!(name, "connection"),
                    FrozenUnit::ValidatorRef { name, .. } => {
                        panic!("a set froze as a validator call: {name}")
                    }
                    _ => {}
                }
            }
        }
    }
    assert_eq!(protocols, 3);
}

#[test]
fn every_error_is_reported_not_the_first() {
    let schema = "@connection = Admin\nprotocol Ops {\n    function restart();\n}\n\n\
                  @connection = []\nprotocol Health {\n    function ping();\n}\n\n\
                  @connection = [public, public]\nprotocol Chat {\n    function send();\n}\n\n\
                  protocol Left {\n    function out();\n}\n";
    let config = "handshake = \"comline@2\"\nconnections = { reports = { wire = { framing = \"datagram@3\" } } }\n";
    let error = error_of(config, schema).expect("the package has errors");
    for code in [
        "connection::bad-set-name",
        "connection::empty-list",
        "connection::duplicate-set",
        "connection::unassigned",
        "connection::unknown-set",
    ] {
        assert!(error.contains(code), "{code} isn't in:\n{error}");
    }
}

#[test]
fn a_refused_encoding_says_whether_it_is_the_handshake_or_the_framing() {
    let schema = "@connection = admin\nprotocol Ops {\n    function restart();\n}\n";

    let error = error_of("handshake = \"comline@1\"\n", schema).unwrap();
    assert!(
        error.contains("connection::unsupported-encoding"),
        "{error}"
    );
    assert!(error.contains("handshake `comline@1`"), "{error}");
    assert!(error.contains("whole_schema"), "{error}");

    let error = error_of(
        "handshake = \"comline@2\"\nwire = { framing = \"datagram@2\" }\n",
        schema,
    )
    .unwrap();
    assert!(
        error.contains("connection::unsupported-encoding"),
        "{error}"
    );
    assert!(error.contains("framing `datagram@2`"), "{error}");
    assert!(error.contains("by_index"), "{error}");
}

#[test]
fn a_bad_set_name_says_why() {
    let named = |name: &str| {
        let schema =
            format!("@connection = {name}\nprotocol Ops {{\n    function restart();\n}}\n");
        error_of("handshake = \"comline@2\"\n", &schema).unwrap()
    };
    assert!(named("Admin").contains("snake_case"));
    assert!(named("loop").contains("keyword of Rust"));
    assert!(named("interface").contains("keyword of TypeScript"));
    assert!(named("serve").contains("connection module"));
}

#[test]
fn a_quoted_name_is_the_annotation_value_error_and_says_to_drop_the_quotes() {
    let schema = "@connection = \"admin\"\nprotocol Ops {\n    function restart();\n}\n";
    let error = error_of("handshake = \"comline@2\"\n", schema).unwrap();
    assert!(
        error.contains("validation::annotation-value-mismatch"),
        "{error}"
    );
    assert!(error.contains("drop the quotes"), "{error}");
}

#[test]
fn a_package_that_never_writes_it_builds_as_before() {
    let schema = "protocol Ops {\n    function restart();\n}\n";
    assert!(error_of(
        "handshake = \"comline@1\"\nwire = { framing = \"datagram@2\" }\n",
        schema
    )
    .is_none());
}

#[test]
fn a_forbidden_annotation_is_the_settings_error() {
    let schema = "@connection = admin\nprotocol Ops {\n    function restart();\n}\n";
    let config = "handshake = \"comline@2\"\nsettings = { annotations = { connection = { allowed = false } } }\n";
    let error = error_of(config, schema).unwrap();
    assert!(error.contains("settings::forbidden"), "{error}");
}
