//! A dependency's pinned `hash` covers its config as well as its schemas
//! (core#252): what its `config.idp` says about the wire format, the
//! encodings it declares and what it leaves unverified is read by whoever
//! depends on it, so a change to any of it has to fail the pin.

use std::path::Path;

use comline_core::package::config::dependency::{DependencyConfig, DependencySource};
use comline_core::package::deps::resolve;

const FRAMING: &str = r#"{
  "name": "com.acme.lines@1",
  "kind": "framing",
  "facts": {
    "calls": { "value": "by_name", "proven_by": ["lines/request"] },
    "directions": { "value": "both", "proven_by": ["lines/request"] },
    "errors": { "value": "by_name", "proven_by": ["lines/error"] },
    "payload": { "value": "opaque", "proven_by": ["lines/request-binary"] },
    "protocol": { "value": "named", "proven_by": ["lines/request"] }
  }
}"#;

const HANDSHAKE: &str = r#"{ "name": "com.acme.hello@1", "kind": "handshake", "facts": {
  "schema_agreement": { "value": true, "proven_by": ["hello/agree"] },
  "encoding_agreement": { "value": true, "proven_by": ["hello/agree"] }
} }"#;

const SCHEMA: &str = "struct Thing {\n    id: u64\n}\n";

const SETTINGS: &str =
    "settings = { annotations = { framing = { allowed = true } unverified = { allowed = true } } }\n";

const ENCODINGS: &str = r#"encodings = {
    "com.acme.lines@1" = { kind = "framing" profile = "profiles/lines.json" }
    "com.acme.hello@1" = { kind = "handshake" profile = "profiles/hello.json" }
}
"#;

/// What a dependency's `config.idp` says besides its header, the parts a
/// test varies.
#[derive(Clone)]
struct Config {
    wire_format: &'static str,
    unverified: Option<&'static str>,
    /// Written between the entries, which changes nothing it says.
    noise: &'static str,
}

impl Config {
    fn base() -> Self {
        Config {
            wire_format: "json@1",
            unverified: None,
            noise: "",
        }
    }

    fn text(&self) -> String {
        let unverified = self
            .unverified
            .map(|r| format!("unverified = \"{r}\"\n"))
            .unwrap_or_default();
        format!(
            "congregation acme\nspecification_version = 1\n{noise}\n\
             {SETTINGS}{noise}{ENCODINGS}wire = {{ format = \"{format}\" }}\n{unverified}",
            noise = self.noise,
            format = self.wire_format,
        )
    }
}

/// The dependency at `dir`: its config, the schema, both profiles, and its
/// lines profile's vectors when asked.
fn write(dir: &Path, config: &Config, handshake: &str, vectors: bool) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("profiles")).unwrap();
    std::fs::write(dir.join("config.idp"), config.text()).unwrap();
    std::fs::write(dir.join("src/api.ids"), SCHEMA).unwrap();
    std::fs::write(dir.join("profiles/lines.json"), FRAMING).unwrap();
    std::fs::write(dir.join("profiles/hello.json"), handshake).unwrap();
    let vectors_file = dir.join("profiles/lines.vectors.json");
    if vectors {
        std::fs::write(vectors_file, "[]").unwrap();
    } else {
        let _ = std::fs::remove_file(vectors_file);
    }
}

fn dependency(path: &str, pin: Option<&str>) -> DependencyConfig {
    DependencyConfig {
        name: "acme".to_string(),
        accept_unverified: Default::default(),
        source: DependencySource::Path {
            path: path.into(),
            hash: pin.map(str::to_string),
        },
    }
}

/// The pin `dir`'s dependency (at `path`, from `root`) has now.
fn pin_of(root: &Path, path: &str) -> String {
    let resolved = resolve(&dependency(path, None), root, &root.join("cache")).unwrap();
    format!("blake3:{}", resolved.content_hash.to_hex())
}

fn resolve_pinned(root: &Path, path: &str, pin: &str) -> Result<(), String> {
    resolve(&dependency(path, Some(pin)), root, &root.join("cache"))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// What `dependency` made of itself before pins covered config: its
/// schemas alone. A pin from then.
const OLD_PIN: &str = "blake3:728e1b2153908105259b94a8d35e91ddf4e589253c8f8c44a1d609545e61f33b";

/// What [`Config::base`] with both profiles and its vectors hashes to now.
/// If this moves while that input stays the same, the hash scheme changed:
/// every pin made before is wrong, so it's a new `CONTENT_HASH_SCHEME` and a
/// breaking change, not a fix to this line. It moves legitimately only when
/// the input does: spec 0.11 made `FRAMING` state `protocol`, which changes
/// its frozen `Encoding` unit (was `blake3:634d212c…`).
const GOLDEN_PIN: &str = "blake3:85584227b207edb2e099f073aef9ce7c76315d3319e6e99769e69c007f4fb4af";

/// Pin the base dependency, change it with `change`, and it no longer
/// verifies, and as a plain mismatch rather than an old pin.
fn assert_change_fails_the_pin(what: &str, change: impl FnOnce(&Path)) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("acme");
    write(&dir, &Config::base(), HANDSHAKE, true);
    let pin = pin_of(tmp.path(), "acme");
    resolve_pinned(tmp.path(), "acme", &pin).expect("it verifies before the change");

    change(&dir);
    let err = resolve_pinned(tmp.path(), "acme", &pin).expect_err(what);
    assert!(err.contains("hash mismatch"), "{what}: {err}");
    assert!(!err.contains("older comline"), "{what}: {err}");
}

#[test]
fn a_changed_wire_format_fails_the_pin() {
    assert_change_fails_the_pin("wire.format", |dir| {
        write(
            dir,
            &Config {
                wire_format: "msgpack@1",
                ..Config::base()
            },
            HANDSHAKE,
            true,
        );
    });
}

#[test]
fn a_changed_encoding_profile_fails_the_pin() {
    // What the handshake establishes drops to encoding agreement alone.
    let weaker = HANDSHAKE.replace(
        r#""schema_agreement": { "value": true, "proven_by": ["hello/agree"] },"#,
        "",
    );
    assert_ne!(weaker, HANDSHAKE);
    assert_change_fails_the_pin("establishes", |dir| {
        write(dir, &Config::base(), &weaker, true)
    });
}

#[test]
fn adding_or_removing_vectors_fails_the_pin() {
    assert_change_fails_the_pin("vectors removed", |dir| {
        write(dir, &Config::base(), HANDSHAKE, false)
    });

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("acme");
    write(&dir, &Config::base(), HANDSHAKE, false);
    let pin = pin_of(tmp.path(), "acme");
    write(&dir, &Config::base(), HANDSHAKE, true);
    let err = resolve_pinned(tmp.path(), "acme", &pin).expect_err("vectors added");
    assert!(err.contains("hash mismatch"), "{err}");
}

#[test]
fn a_changed_package_scope_unverified_fails_the_pin() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("acme");
    let pins: Vec<String> = [None, Some("a legacy peer"), Some("another reason")]
        .into_iter()
        .map(|unverified| {
            write(
                &dir,
                &Config {
                    unverified,
                    ..Config::base()
                },
                HANDSHAKE,
                true,
            );
            pin_of(tmp.path(), "acme")
        })
        .collect();
    assert_ne!(pins[0], pins[1], "added");
    assert_ne!(pins[1], pins[2], "reworded");

    // And a pin made without it fails once it is there.
    write(
        &dir,
        &Config {
            unverified: Some("a legacy peer"),
            ..Config::base()
        },
        HANDSHAKE,
        true,
    );
    let err = resolve_pinned(tmp.path(), "acme", &pins[0]).expect_err("unverified added");
    assert!(err.contains("hash mismatch"), "{err}");
}

#[test]
fn whitespace_comments_and_key_order_in_config_dont_change_the_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("acme");
    write(&dir, &Config::base(), HANDSHAKE, true);
    let pin = pin_of(tmp.path(), "acme");

    let noisy = Config {
        noise: "\n\n// what the package is for\n   \n",
        ..Config::base()
    };
    write(&dir, &noisy, HANDSHAKE, true);
    resolve_pinned(tmp.path(), "acme", &pin).expect("comments and blank lines");

    // The same entries, in another order.
    let reordered = format!(
        "congregation acme\nspecification_version = 1\nwire = {{ format = \"json@1\" }}\n\
         {ENCODINGS}{SETTINGS}"
    );
    std::fs::write(dir.join("config.idp"), reordered).unwrap();
    resolve_pinned(tmp.path(), "acme", &pin).expect("keys in another order");

    // CRLF line endings in `config.idp`.
    let crlf = std::fs::read_to_string(dir.join("config.idp"))
        .unwrap()
        .replace('\n', "\r\n");
    std::fs::write(dir.join("config.idp"), crlf).unwrap();
    resolve_pinned(tmp.path(), "acme", &pin).expect("CRLF line endings");
}

#[test]
fn where_the_dependency_lives_doesnt_change_the_hash() {
    let tmp = tempfile::tempdir().unwrap();
    write(&tmp.path().join("acme"), &Config::base(), HANDSHAKE, true);
    write(
        &tmp.path().join("vendor/deeper/acme"),
        &Config::base(),
        HANDSHAKE,
        true,
    );
    let here = pin_of(tmp.path(), "acme");
    assert_eq!(here, pin_of(tmp.path(), "vendor/deeper/acme"));
    assert_eq!(here, pin_of(tmp.path(), "./vendor/../acme"));

    // Another consumer, somewhere else, with the dependency beside it.
    let other = tempfile::tempdir().unwrap();
    write(
        &other.path().join("libs/acme"),
        &Config::base(),
        HANDSHAKE,
        true,
    );
    assert_eq!(here, pin_of(other.path(), "libs/acme"));
}

#[test]
fn a_pin_from_before_config_was_covered_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    write(&tmp.path().join("acme"), &Config::base(), HANDSHAKE, true);
    let err = resolve_pinned(tmp.path(), "acme", OLD_PIN).expect_err("an old pin can't verify");
    assert!(
        err.contains("this pin was made by an older comline"),
        "{err}"
    );
    assert!(
        err.contains(&format!("hash = \"{GOLDEN_PIN}\"")),
        "names the pin to write: {err}"
    );
    assert!(!err.contains("tampered"), "{err}");

    // Following it re-pins.
    resolve_pinned(tmp.path(), "acme", GOLDEN_PIN).expect("the new pin verifies");

    // A pin that is neither is the plain mismatch.
    let wrong = format!("blake3:{}", "0".repeat(64));
    let err = resolve_pinned(tmp.path(), "acme", &wrong).unwrap_err();
    assert!(err.contains("hash mismatch"), "{err}");
    assert!(!err.contains("older comline"), "{err}");
}

#[test]
fn the_hash_of_a_known_dependency_is_pinned_down() {
    let tmp = tempfile::tempdir().unwrap();
    write(&tmp.path().join("acme"), &Config::base(), HANDSHAKE, true);
    assert_eq!(
        pin_of(tmp.path(), "acme"),
        GOLDEN_PIN,
        "the hash scheme drifted"
    );
}
