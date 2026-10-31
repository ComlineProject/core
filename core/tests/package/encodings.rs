//! `encodings = { ... }` in `config.idp`: a package declares formats,
//! framings and handshakes beyond the official ones, each with a profile,
//! and it and its dependents can then use them (core#120).

use std::path::Path;

use comline_core::encodings::{EncodingKind, EncodingMapping};
use comline_core::package::build::{compile_package, PackageSources};
use comline_core::package::config::ir::frozen::FrozenUnit as ConfigUnit;

const FRAMING_PROFILE: &str = r#"{
  "name": "com.acme.lines@1",
  "kind": "framing",
  "facts": {
    "calls": { "value": "by_name", "proven_by": ["lines/request"] },
    "errors": { "value": "by_name", "proven_by": ["lines/error"] }
  }
}"#;

const FORMAT_PROFILE: &str = r#"{
  "name": "com.acme.cbor@1",
  "kind": "format",
  "facts": {
    "struct_fields": { "value": "by_name", "proven_by": [] },
    "unknown_field": { "value": "ignored", "proven_by": [] },
    "missing_optional": { "value": "absent", "proven_by": [] },
    "enum_variants": { "value": "by_name", "proven_by": [] },
    "unknown_variant": { "value": "rejected", "proven_by": [] },
    "integer_width": { "value": "by_value", "proven_by": [] }
  }
}"#;

const LINES: &str = r#"encodings = {
    "com.acme.lines@1" = {
        kind = "framing"
        profile = "profiles/lines.json"
        map = {
            rust = { type = "acme_lines::Lines" package = "acme-lines" version = "0.3" }
            typescript = { type = "LinesFraming" }
        }
    }
}
"#;

/// A package in `dir`: `config.idp` (after the header), `src/api.ids`, and
/// each `(path, contents)` file.
fn write_package(dir: &Path, name: &str, config: &str, schema: &str, files: &[(&str, &str)]) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("config.idp"),
        format!(
            "congregation {name}\nspecification_version = 1\n\n\
             settings = {{ annotations = {{ framing = {{ allowed = true }} }} }}\n\n{config}"
        ),
    )
    .unwrap();
    std::fs::write(dir.join("src/api.ids"), schema).unwrap();
    for (path, contents) in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
}

fn compile_one(config: &str, schema: &str, files: &[(&str, &str)]) -> eyre::Result<Vec<ConfigUnit>> {
    let tmp = tempfile::tempdir().unwrap();
    write_package(tmp.path(), "app", config, schema, files);
    Ok(compile_package(tmp.path())?.config_frozen.unwrap())
}

fn error_of(config: &str, schema: &str, files: &[(&str, &str)]) -> String {
    format!("{:?}", compile_one(config, schema, files).unwrap_err())
}

const PLAIN: &str = "struct S {\n    a: u32\n}\n";

#[test]
fn a_package_declares_a_framing_and_uses_it() {
    let schema = "@framing = \"com.acme.lines@1\"\nprotocol P {\n    function f();\n}\n";
    let config = compile_one(LINES, schema, &[("profiles/lines.json", FRAMING_PROFILE)]).unwrap();

    let encoding = config.iter().find(|u| matches!(u, ConfigUnit::Encoding { .. })).unwrap();
    let ConfigUnit::Encoding { name, kind, profile_hash, map } = encoding else { unreachable!() };
    assert_eq!(name, "com.acme.lines@1");
    assert_eq!(*kind, EncodingKind::Framing);
    assert_eq!(*profile_hash, blake3::hash(FRAMING_PROFILE.as_bytes()).to_hex().to_string());
    assert_eq!(
        map["rust"],
        EncodingMapping {
            ty: "acme_lines::Lines".into(),
            package: Some("acme-lines".into()),
            version: Some("0.3".into()),
        }
    );
    assert_eq!(map["typescript"], EncodingMapping { ty: "LinesFraming".into(), package: None, version: None });

    // The dictionary form too, and as the package's `wire.framing`.
    let schema = "@framing = { name = \"com.acme.lines@1\" }\nprotocol P {\n    function f();\n}\n";
    compile_one(LINES, schema, &[("profiles/lines.json", FRAMING_PROFILE)]).unwrap();
    let config = format!("{LINES}wire = {{ framing = \"com.acme.lines@1\" }}\n");
    compile_one(&config, PLAIN, &[("profiles/lines.json", FRAMING_PROFILE)]).unwrap();
}

#[test]
fn an_encoding_round_trips_through_the_cas_encoding() {
    let config = compile_one(LINES, PLAIN, &[("profiles/lines.json", FRAMING_PROFILE)]).unwrap();
    let mut bytes = vec![];
    ciborium::ser::into_writer(&config, &mut bytes).unwrap();
    let back: Vec<ConfigUnit> = ciborium::de::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(back, config);
}

#[test]
fn an_undeclared_framing_is_an_error_never_a_default() {
    // cli#81: an unknown framing used to compile and generate as datagram.
    for framing in ["nonsense", "com.acme.lines@1"] {
        let schema = format!("@framing = \"{framing}\"\nprotocol P {{\n    function f();\n}}\n");
        let err = error_of("", &schema, &[]);
        assert!(err.contains(&format!("framing `{framing}` isn't")) || err.contains(&format!("got \"{framing}\"")), "{framing}: {err}");
        assert!(!err.contains("settings::forbidden"), "{err}");
    }
    let err = error_of("", "@framing = \"com.acme.lines@1\"\nprotocol P {\n    function f();\n}\n", &[]);
    assert!(err.contains("framing `com.acme.lines@1` isn't declared"), "{err}");

    // Declared as a format isn't declared as a framing.
    let cbor = "encodings = {\n    \"com.acme.cbor@1\" = { kind = \"format\" profile = \"cbor.json\" }\n}\n";
    let schema = "@framing = \"com.acme.cbor@1\"\nprotocol P {\n    function f();\n}\n";
    let err = error_of(cbor, schema, &[("cbor.json", FORMAT_PROFILE)]);
    assert!(err.contains("framing `com.acme.cbor@1` isn't declared"), "{err}");

    // In `wire`, with no protocol to carry it, still an error.
    let err = error_of("wire = { framing = \"com.acme.lines@1\" }\n", PLAIN, &[]);
    assert!(err.contains("`wire.framing`: framing `com.acme.lines@1` isn't declared"), "{err}");
    let err = error_of("wire = { format = \"com.acme.cbor@1\" }\n", PLAIN, &[]);
    assert!(err.contains("`wire.format`: format `com.acme.cbor@1` isn't declared"), "{err}");
}

#[test]
fn names_are_namespaced_and_versioned() {
    let profile = [("profiles/lines.json", FRAMING_PROFILE)];
    for (name, expected) in [
        ("lines@1", "bare names are reserved to official encodings; use a namespaced name such as com.acme.lines@1"),
        ("json@1", "bare names are reserved to official encodings"),
        ("com.acme.lines", "needs a version"),
        ("Com.Acme.Lines@1", "isn't a valid name"),
        ("com..lines@1", "isn't a valid name"),
    ] {
        let config = LINES.replace("\"com.acme.lines@1\" =", &format!("\"{name}\" ="));
        let err = error_of(&config, PLAIN, &profile);
        assert!(err.contains(expected), "{name}: {err}");
        assert!(err.contains("config::encoding-name"), "{name}: {err}");
    }
    // An unquoted bare key reads the same, and is refused the same.
    let config = LINES.replace("\"com.acme.lines@1\" =", "lines@1 =");
    assert!(error_of(&config, PLAIN, &profile).contains("bare names are reserved"));
}

#[test]
fn profiles_are_read_and_checked() {
    let err = error_of(LINES, PLAIN, &[]);
    assert!(err.contains("its profile `profiles/lines.json` can't be read"), "{err}");
    assert!(err.contains("config::encoding-profile"), "{err}");
    // The diagnostic points into config.idp.
    assert!(err.contains("config.idp"), "{err}");

    let err = error_of(LINES, PLAIN, &[("profiles/lines.json", "{ not json")]);
    assert!(err.contains("is malformed: it isn't a profile"), "{err}");

    let wrong_fact = FRAMING_PROFILE.replace("\"by_name\", \"proven_by\": [\"lines/request\"]", "\"by_magic\", \"proven_by\": []");
    let err = error_of(LINES, PLAIN, &[("profiles/lines.json", &wrong_fact)]);
    assert!(err.contains("its `calls` is `by_magic`"), "{err}");

    let err = error_of(LINES, PLAIN, &[("profiles/lines.json", &FORMAT_PROFILE.replace("com.acme.cbor@1", "com.acme.lines@1"))]);
    assert!(err.contains("encoding `com.acme.lines@1` is a framing, but its profile `profiles/lines.json` is a format profile"), "{err}");

    let err = error_of(LINES, PLAIN, &[("profiles/lines.json", &FRAMING_PROFILE.replace("com.acme.lines@1", "com.acme.other@1"))]);
    assert!(err.contains("is for `com.acme.other@1`"), "{err}");

    let outside = LINES.replace("profiles/lines.json", "../lines.json");
    assert!(error_of(&outside, PLAIN, &[]).contains("must be a path inside the package"));

    let no_profile = LINES.replace("profile = \"profiles/lines.json\"", "");
    assert!(error_of(&no_profile, PLAIN, &[]).contains("needs a `profile`"));
}

#[test]
fn an_in_memory_package_cannot_read_a_profile() {
    let config = format!("congregation app\nspecification_version = 1\n{LINES}");
    let err = PackageSources::new().config(config).schema(["app"], PLAIN).compile().unwrap_err();
    assert!(err.to_string().contains("can't be read without a package directory"), "{err}");
}

/// A dependency declares a format; its dependent speaks it in `wire`.
#[cfg(feature = "deps")]
#[test]
fn a_dependent_uses_its_dependencys_format() {
    let tmp = tempfile::tempdir().unwrap();
    let cbor = r#"encodings = {
    "com.acme.cbor@1" = { kind = "format" profile = "cbor.json" map = { rust = { type = "acme_cbor::Cbor" } } }
}
"#;
    write_package(&tmp.path().join("acme"), "acme", cbor, PLAIN, &[("cbor.json", FORMAT_PROFILE)]);

    let deps = "dependencies = {\n    acme = { path = \"../acme\" }\n}\n";
    let config = format!("{deps}wire = {{ format = \"com.acme.cbor@1\" }}\n");
    write_package(&tmp.path().join("app"), "app", &config, PLAIN, &[]);
    let context = compile_package(&tmp.path().join("app")).unwrap();
    let frozen = context.config_frozen.unwrap();
    assert_eq!(comline_core::package::config::ir::frozen::wire_format(&frozen), "com.acme.cbor@1");
    // The declaration stays the dependency's.
    assert!(!frozen.iter().any(|u| matches!(u, ConfigUnit::Encoding { .. })));

    // Without the dependency, the same name is undeclared.
    write_package(&tmp.path().join("alone"), "alone", "wire = { format = \"com.acme.cbor@1\" }\n", PLAIN, &[]);
    let err = format!("{:?}", compile_package(&tmp.path().join("alone")).unwrap_err());
    assert!(err.contains("format `com.acme.cbor@1` isn't declared"), "{err}");
}

/// The official encodings are declared by the embedded `comline` package,
/// which every package sees: each name and alias works where it did, folds
/// onto the same canonical spelling, and none of the official declarations
/// land in the package's own frozen config.
#[test]
fn every_official_name_and_alias_resolves_as_before() {
    for (written, canonical) in [
        ("jsonrpc", "jsonrpc"),
        ("json-rpc", "jsonrpc"),
        ("jsonrpc-2.0", "jsonrpc"),
        ("jsonrpc@1", "jsonrpc"),
        ("datagram", "datagram"),
        ("datagram@1", "datagram"),
        ("comline.datagram", "datagram"),
        ("datagram@2", "datagram@2"),
    ] {
        let config = compile_one(&format!("wire = {{ framing = \"{written}\" }}\n"), PLAIN, &[]).unwrap();
        assert!(
            config.contains(&ConfigUnit::Wire { format: None, framing: Some(canonical.to_string()) }),
            "{written}: {config:?}"
        );
        assert!(!config.iter().any(|u| matches!(u, ConfigUnit::Encoding { .. })), "{written}: {config:?}");

        let schema = format!("@framing = \"{written}\"\nprotocol P {{\n    function f();\n}}\n");
        compile_one("", &schema, &[]).unwrap_or_else(|e| panic!("{written}: {e:?}"));
    }
    for format in ["packed@1", "json@1", "msgpack@1"] {
        let config = compile_one(&format!("wire = {{ format = \"{format}\" }}\n"), PLAIN, &[]).unwrap();
        assert!(config.contains(&ConfigUnit::Wire { format: Some(format.to_string()), framing: None }), "{format}");
    }
    // `none` (and `@unverified`, which it needs) is `handshake.rs`'s.
    let config = compile_one("handshake = \"comline@1\"\n", PLAIN, &[]).unwrap();
    assert!(config.contains(&ConfigUnit::Handshake("comline@1".to_string())), "{config:?}");
    compile_one("", "@handshake = \"comline@1\"\nprotocol P {\n    function f();\n}\n", &[]).unwrap();
}

/// What an unknown official-looking name is told comes from the official
/// package's declarations.
#[test]
fn an_unknown_name_lists_the_declared_official_ones() {
    let err = error_of("wire = { framing = \"datagram@3\" }\n", PLAIN, &[]);
    assert!(err.contains("expected one of `jsonrpc`, `datagram`, `datagram@2`"), "{err}");
    let err = error_of("wire = { format = \"packed\" }\n", PLAIN, &[]);
    assert!(err.contains("expected one of `packed@1`, `json@1`, `msgpack@1`"), "{err}");
}

/// Bare names are the official package's by where it comes from, not by
/// its name: a user package called `comline` is refused one like any other.
#[test]
fn a_user_package_named_comline_cannot_declare_a_bare_name() {
    let tmp = tempfile::tempdir().unwrap();
    let config = LINES.replace("\"com.acme.lines@1\" =", "\"lines@1\" =").replace("com.acme.lines@1", "lines@1");
    write_package(tmp.path(), "comline", &config, PLAIN, &[("profiles/lines.json", &FRAMING_PROFILE.replace("com.acme.lines@1", "lines@1"))]);
    let err = format!("{:?}", compile_package(tmp.path()).unwrap_err());
    assert!(err.contains("bare names are reserved to official encodings"), "{err}");
}
