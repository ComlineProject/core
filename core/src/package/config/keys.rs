//! What `config.idp` accepts: every key, with its type, its allowed values,
//! its default and a line of prose.
//!
//! The reader (`ir::interpreter::freezing`, `dependency`) is a `match` on key
//! names, so nothing could say which keys exist, and every editor kept its own
//! copy that went stale (comline-vscode#31). [`keys`] is that answer as data,
//! and [`json`] is it as the committed `schemas/config-idp.json`, which the
//! editor extension generates its completion and hover from. The enumerations
//! come from where core declares them (the official encodings, the handshake's
//! parameters, the editions, the guarantees), so only the prose is written
//! here. The tests hold the table to the reader: a key the reader takes that
//! the table lacks, or the other way round, fails them.
//!
//! Not every key is described to the bottom: `settings` is a dictionary the
//! settings registry ([`crate::settings`]) describes, and the language server
//! serves inside it.

use serde_json::{json, Value};

use crate::edition::Edition;
use crate::encodings::{official, ProfileKind};
use crate::schema::idl::annotations::{
    COMLINE2_PARAMS, DEFAULT_FORMAT, DEFAULT_FRAMING, DEFAULT_HANDSHAKE, IMPLICIT_HANDSHAKE,
};

/// The version of the JSON this module writes ([`json`]), for a reader to
/// refuse a shape it doesn't know.
pub const SCHEMA_VERSION: u32 = 1;

/// One key of `config.idp`, at any depth.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigKey {
    /// The key as written, or for an entry the user names, what names it in
    /// angle brackets: `<dependency name>`.
    pub key: String,
    /// One line, plain text.
    pub summary: String,
    /// The full explanation. Markdown, but for backticks it is plain.
    pub description: String,
    /// Whether the dictionary it is in is wrong without it.
    pub required: bool,
    pub value: ConfigShape,
}

/// What a key's value is.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigShape {
    /// A number; `values` are the only ones allowed, where it is a closed set.
    Integer { values: Vec<i64> },
    /// Any quoted string.
    String,
    /// A quoted string from a closed set, `default` the one that applies where
    /// it's left out. `namespaced` is whether a name a package declares under
    /// `encodings` (`com.acme.x@1`) is allowed too.
    Choice {
        values: Vec<String>,
        default: Option<String>,
        namespaced: bool,
    },
    /// `[ item, ... ]`.
    List(Box<ConfigShape>),
    /// `{ key = value ... }` with these keys, and no others.
    Dictionary(Vec<ConfigKey>),
    /// `{ name = value ... }` with names the user chooses, each value the
    /// shape of this entry, whose `key` says what names it.
    Map(Box<ConfigKey>),
    /// Any one of these.
    OneOf(Vec<ConfigShape>),
    /// A dictionary of the settings registry's keys ([`crate::settings`]),
    /// which this table doesn't repeat.
    Settings,
}

impl ConfigKey {
    fn new(key: &str, summary: &str, description: String, value: ConfigShape) -> Self {
        ConfigKey {
            key: key.into(),
            summary: summary.into(),
            description,
            required: false,
            value,
        }
    }

    fn required(mut self) -> Self {
        self.required = true;
        self
    }

    fn to_json(&self) -> Value {
        let mut key = json!({
            "key": self.key,
            "summary": self.summary,
            "description": self.description,
            "value": self.value.to_json(),
        });
        if self.required {
            key["required"] = true.into();
        }
        key
    }
}

impl ConfigShape {
    fn to_json(&self) -> Value {
        match self {
            ConfigShape::Integer { values } if values.is_empty() => json!({ "kind": "integer" }),
            ConfigShape::Integer { values } => json!({ "kind": "integer", "values": values }),
            ConfigShape::String => json!({ "kind": "string" }),
            ConfigShape::Choice {
                values,
                default,
                namespaced,
            } => {
                let mut choice = json!({ "kind": "choice", "values": values });
                if let Some(default) = default {
                    choice["default"] = default.clone().into();
                }
                if *namespaced {
                    choice["namespaced"] = true.into();
                }
                choice
            }
            ConfigShape::List(item) => json!({ "kind": "list", "item": item.to_json() }),
            ConfigShape::Dictionary(keys) => {
                json!({ "kind": "dictionary", "fields": keys.iter().map(ConfigKey::to_json).collect::<Vec<_>>() })
            }
            ConfigShape::Map(entry) => json!({ "kind": "map", "entry": entry.to_json() }),
            ConfigShape::OneOf(alternatives) => {
                json!({ "kind": "oneOf", "alternatives": alternatives.iter().map(ConfigShape::to_json).collect::<Vec<_>>() })
            }
            ConfigShape::Settings => json!({ "kind": "settings" }),
        }
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
}

/// An official encoding's names, one of them the default.
fn official_choice(kind: ProfileKind, default: Option<&str>) -> ConfigShape {
    let values = official::names(kind)
        .into_iter()
        .map(String::from)
        .collect();
    ConfigShape::Choice {
        values,
        default: default.map(String::from),
        namespaced: true,
    }
}

fn dictionary_entry(
    key: &str,
    summary: &str,
    description: &str,
    fields: Vec<ConfigKey>,
) -> ConfigKey {
    ConfigKey::new(
        key,
        summary,
        description.into(),
        ConfigShape::Dictionary(fields),
    )
}

/// The keys `config.idp` takes at its root.
pub fn keys() -> Vec<ConfigKey> {
    let editions: Vec<i64> = Edition::ALL.iter().map(|e| i64::from(e.number())).collect();
    let edition = ConfigKey::new(
        "specification_version",
        "The edition of the schema language this package is written in",
        format!(
            "The edition of the schema language this package is written in, as a number; this core \
             knows {}. A change that would break schemas already written ships in a new edition, and a \
             package keeps its behaviour until it moves to it. Every package states one.",
            Edition::known()
        ),
        ConfigShape::Integer { values: editions },
    );

    let id = ConfigKey::new(
        "id",
        "The package's identity, written by comline build",
        "The package's identity for the compatibility check: 32 lowercase hex digits. \
         `comline new` and `comline build` write one when it's missing. A rename keeps it, a \
         copy must change it, and changing it is a major change."
            .into(),
        ConfigShape::String,
    );

    let code_generation = dictionary_entry(
        "code_generation",
        "What this package can be compiled to",
        "Declares what this package can be compiled to. Its only key is `languages`. \
         Per-target output (directory, layout, package versions) lives in `comline.toml`'s \
         `[generate]` table, not here.",
        vec![ConfigKey::new(
            "languages",
            "Targets this package can generate, checked by the generators",
            "The `language#version` targets this package declares it can generate. \
             Each entry's value must be an empty dict (`lang#ver = {}`: it takes no options). \
             A target is checked against the generators the CLI has when the package is generated, \
             not here."
                .into(),
            ConfigShape::Map(Box::new(dictionary_entry(
                "<language>#<version>",
                "A target, taking no options",
                "A language and the version of its generator, written `rust#1.70.0`. The value is \
                 `{}`: a declared target takes no options.",
                vec![],
            ))),
        )],
    );

    let wire = dictionary_entry(
        "wire",
        "The wire format and framing both ends speak",
        &format!(
            "The package's wire defaults, which both ends of a connection must agree on: \
             `wire = {{ format = \"{DEFAULT_FORMAT}\" framing = \"{DEFAULT_FRAMING}\" }}`. Leaving \
             out either key gives its default, shown there. A protocol's own `@framing` wins over \
             `framing`."
        ),
        vec![
            ConfigKey::new(
                "format",
                "How values are encoded",
                format!(
                    "How values are encoded, `\"{DEFAULT_FORMAT}\"` where left out: fixed-width fields \
                     and no tags for `packed@1`, UTF-8 JSON for `json@1`, MessagePack for \
                     `msgpack@1`, or a namespaced format the package or a dependency declares \
                     under `encodings`. JSON-RPC protocols always speak JSON."
                ),
                official_choice(ProfileKind::Format, Some(DEFAULT_FORMAT)),
            ),
            ConfigKey::new(
                "framing",
                "How calls are framed",
                format!(
                    "How calls are framed, for every protocol with no `@framing` of its own; \
                     `\"{DEFAULT_FRAMING}\"` where left out. The `datagram` framings are Comline's \
                     own binary one: `datagram@3` and `datagram@2` are a connection, where either \
                     end can call and several protocols share one transport (`datagram@3` addresses \
                     a call by its wire number), `datagram@1` is lockstep, one protocol per \
                     transport. `jsonrpc@1` is JSON-RPC 2.0. Or a namespaced framing a package \
                     declares under `encodings`."
                ),
                official_choice(ProfileKind::Framing, Some(DEFAULT_FRAMING)),
            ),
        ],
    );

    let handshake_names = official_choice(ProfileKind::Handshake, None);
    let mut handshake_fields = vec![ConfigKey::new(
        "name",
        "The handshake",
        "Which handshake the package's protocols run. The same as the plain form: \
         `handshake = \"comline@2\"` is `{ name = \"comline@2\" }`."
            .into(),
        handshake_names.clone(),
    )
    .required()];
    for param in COMLINE2_PARAMS {
        handshake_fields.push(ConfigKey::new(
            param.key,
            param.summary,
            format!(
                "{} A parameter of `comline@2`, `{}` where left out. {}",
                param.doc,
                param.values[0],
                if param.wire {
                    "Both ends must agree on it."
                } else {
                    "Each end applies it on its own."
                }
            ),
            ConfigShape::Choice {
                values: strings(param.values),
                default: Some(param.values[0].to_string()),
                namespaced: false,
            },
        ));
    }
    let handshake = ConfigKey::new(
        "handshake",
        "The handshake the package's protocols run",
        format!(
            "The handshake each protocol of the package runs when it names none in `@handshake`: \
             a name, `handshake = \"comline@2\"`, or a dictionary when the handshake takes \
             parameters, `handshake = {{ name = \"comline@2\" scope = \"package\" }}`. Left out, \
             it is `{DEFAULT_HANDSHAKE}`, or `{IMPLICIT_HANDSHAKE}` in a package whose protocols \
             can't all run `comline@2`. A protocol's own `@handshake` replaces the package's \
             whole. Only `comline@2` takes parameters."
        ),
        ConfigShape::OneOf(vec![
            handshake_names,
            ConfigShape::Dictionary(handshake_fields),
        ]),
    );

    let guarantees: Vec<ConfigKey> = comline_catalog::sets::GUARANTEES
        .iter()
        .map(|id| {
            ConfigKey::new(
                id,
                "Why the package gives this guarantee up",
                format!("The reason the package gives up `{id}`, for the protocols that take it from the package."),
                ConfigShape::String,
            )
        })
        .collect();
    let unverified = ConfigKey::new(
        "unverified",
        "Guarantees the package gives up, with a reason",
        "Acknowledges, for the whole package, the guarantees its protocols give up through the \
         package's handshake, wire or settings: one reason, `unverified = \"the gateway pins \
         schemas by contract\"`, or a reason for each guarantee, \
         `unverified = { schema_agreement = \"...\" }`. A protocol's own `@unverified` wins where \
         both say."
            .into(),
        ConfigShape::OneOf(vec![
            ConfigShape::String,
            ConfigShape::Dictionary(guarantees),
        ]),
    );

    let settings = ConfigKey::new(
        "settings",
        "Authoring policy for every schema in the package",
        "Package-wide authoring policy, applying to every schema by default: which annotations \
         and validators are allowed, bounds, and diagnostic levels. Its keys are the settings \
         registry's, which the language server completes and explains inside the block. A \
         dictionary, or a reference to another package's named settings schema."
            .into(),
        ConfigShape::Settings,
    );

    let language_mapping = dictionary_entry(
        "<language>",
        "How one language implements the encoding",
        "The type that implements the encoding in this language, and optionally the package and \
         version it comes from.",
        vec![
            ConfigKey::new(
                "type",
                "The implementing type",
                "The type implementing the encoding.".into(),
                ConfigShape::String,
            )
            .required(),
            ConfigKey::new(
                "package",
                "The package the type comes from",
                "The package the type comes from.".into(),
                ConfigShape::String,
            ),
            ConfigKey::new(
                "version",
                "The package's version",
                "The version of that package.".into(),
                ConfigShape::String,
            ),
        ],
    );
    let kinds = [
        ProfileKind::Format,
        ProfileKind::Framing,
        ProfileKind::Handshake,
    ];
    let encodings = ConfigKey::new(
        "encodings",
        "Encodings the package declares",
        "Formats, framings and handshakes beyond the official ones, keyed by a namespaced name \
         (`com.acme.x@1`). Each has a profile, a file in the package that states what it \
         establishes, and may suggest how each language implements it."
            .into(),
        ConfigShape::Map(Box::new(dictionary_entry(
            "<name>@<number>",
            "An encoding and its profile",
            "A namespaced name, `com.acme.x@1`, with the kind of encoding and its profile.",
            vec![
                ConfigKey::new(
                    "kind",
                    "format, framing or handshake",
                    "What the encoding is.".into(),
                    ConfigShape::Choice {
                        values: kinds.iter().map(|k| k.as_str().to_string()).collect(),
                        default: None,
                        namespaced: false,
                    },
                )
                .required(),
                ConfigKey::new(
                    "profile",
                    "The profile's path, inside the package",
                    "The path of the encoding's profile (JSON), relative to the package and inside \
                     it. Its kind and name must be this entry's. Conformance vectors sit beside \
                     it: `profiles/x.json`'s are `profiles/x.vectors.json`."
                        .into(),
                    ConfigShape::String,
                )
                .required(),
                ConfigKey::new(
                    "map",
                    "Suggested implementations, by language",
                    "Where each language finds a type implementing the encoding, keyed by language."
                        .into(),
                    ConfigShape::Map(Box::new(language_mapping)),
                ),
            ],
        ))),
    );

    let publish_registries = ConfigKey::new(
        "publish_registries",
        "Named publish destinations, keyed by name you choose",
        "Named publish destinations for this package, keyed by a name you choose.".into(),
        ConfigShape::Map(Box::new(ConfigKey::new(
            "<registry name>",
            "A publish registry reference",
            "A string, identifier, or namespaced reference to a registry, or a dictionary with a \
             `uri`."
                .into(),
            ConfigShape::OneOf(vec![
                ConfigShape::String,
                ConfigShape::Dictionary(vec![ConfigKey::new(
                    "uri",
                    "The registry URI",
                    "The registry URI.".into(),
                    ConfigShape::String,
                )]),
            ]),
        ))),
    );

    let dependency_fields = vec![
        ConfigKey::new(
            "path",
            "Local path, which makes this a Path dependency",
            "Local filesystem path to a sibling package, which makes this a Path dependency (no \
             declared version; its version is read from the sibling's own last build)."
                .into(),
            ConfigShape::String,
        ),
        ConfigKey::new(
            "version",
            "Declared version (Git and Registry dependencies)",
            "Declared version. Required for Git and Registry dependencies (omit for Path \
             dependencies)."
                .into(),
            ConfigShape::String,
        ),
        ConfigKey::new(
            "uri",
            "Source URI (Git and Registry dependencies)",
            "Source URI. Required for Git and Registry dependencies.".into(),
            ConfigShape::String,
        ),
        ConfigKey::new(
            "commit",
            "Pinned commit, which makes this a Git dependency",
            "Pinned commit hash. Its presence makes this a Git dependency (needs `version` and \
             `uri` too)."
                .into(),
            ConfigShape::String,
        ),
        ConfigKey::new(
            "hash",
            "Optional integrity hash",
            "Optional integrity hash (e.g. `blake3:...`). Valid on any dependency kind.".into(),
            ConfigShape::String,
        ),
        ConfigKey::new(
            "signature",
            "Optional signature (Registry dependencies only)",
            "Optional signature. Registry dependencies only.".into(),
            ConfigShape::String,
        ),
        ConfigKey::new(
            "accept_unverified",
            "The dependency's unverified protocols this package accepts",
            "The dependency's `@unverified` protocols this package accepts, each with the \
             guarantees it was accepted for: `accept_unverified = { Billing = [\"schema_agreement\"] }`. \
             If the dependency gives up more, the build stops until it's accepted again; \
             `comline accept` writes it."
                .into(),
            ConfigShape::Map(Box::new(ConfigKey::new(
                "<protocol>",
                "The guarantees accepted for one protocol",
                "The guarantees accepted for this protocol of the dependency.".into(),
                ConfigShape::List(Box::new(ConfigShape::Choice {
                    values: comline_catalog::sets::GUARANTEES.iter().map(|g| g.to_string()).collect(),
                    default: None,
                    namespaced: false,
                })),
            ))),
        ),
    ];
    let dependencies = ConfigKey::new(
        "dependencies",
        "This package's dependencies, keyed by name you choose",
        "This package's dependencies, keyed by a name you choose. It can't be `self`, `parent`, \
         `package` or `std`."
            .into(),
        ConfigShape::Map(Box::new(dictionary_entry(
            "<dependency name>",
            "A Path, Git, or Registry dependency",
            "A Path dependency (`path`), a Git dependency (`version` + `uri` + `commit`), or a \
             Registry dependency (`version` + `uri`): which fields are present determines the \
             kind.",
            dependency_fields,
        ))),
    );

    vec![
        edition,
        id,
        code_generation,
        wire,
        handshake,
        unverified,
        settings,
        encodings,
        publish_registries,
        dependencies,
    ]
}

/// [`keys`] as the JSON committed in `schemas/config-idp.json`.
pub fn json() -> Value {
    json!({
        "$comment": "Generated from comline_core::package::config::keys; regenerate with \
                     `COMLINE_BLESS=1 cargo test -p comline-core --lib config::keys`.",
        "version": SCHEMA_VERSION,
        "keys": keys().iter().map(ConfigKey::to_json).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn schema_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../schemas/config-idp.json")
    }

    /// `schemas/config-idp.json` is generated from [`keys`]. Regenerate it
    /// with `COMLINE_BLESS=1 cargo test -p comline-core --lib config::keys`.
    #[test]
    fn the_committed_schema_matches_the_table() {
        let generated = serde_json::to_string_pretty(&json()).unwrap() + "\n";
        let path = schema_path();
        if std::env::var_os("COMLINE_BLESS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &generated).unwrap();
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            committed == generated,
            "{} is out of date with comline_core::package::config::keys - regenerate it with \
             `COMLINE_BLESS=1 cargo test -p comline-core --lib config::keys`",
            path.display()
        );
    }

    #[test]
    fn every_key_has_prose() {
        fn check(key: &ConfigKey) {
            assert!(
                !key.summary.is_empty() && !key.description.is_empty(),
                "{}",
                key.key
            );
            assert!(
                !key.summary.contains('\n'),
                "{}: a summary is one line",
                key.key
            );
            check_shape(&key.value);
        }
        fn check_shape(shape: &ConfigShape) {
            match shape {
                ConfigShape::Dictionary(keys) => keys.iter().for_each(check),
                ConfigShape::Map(entry) => check(entry),
                ConfigShape::OneOf(alternatives) => alternatives.iter().for_each(check_shape),
                ConfigShape::List(item) => check_shape(item),
                _ => {}
            }
        }
        keys().iter().for_each(check);
    }
}

/// The table held to the reader.
#[cfg(all(test, feature = "compiler"))]
mod reader_tests {
    use super::*;
    use crate::package::config::ir::context::Origin;
    use crate::package::config::ir::interpreter::{freezing, ProjectInterpreter};

    /// A name for an entry the user names, from what names it.
    fn example_name(placeholder: &str) -> String {
        match placeholder {
            "<language>#<version>" => "rust#1.70.0".into(),
            // The official package declares bare names, and has these profiles.
            "<name>@<number>" => "packed@1".into(),
            other => other
                .trim_matches(|c| c == '<' || c == '>')
                .replace(' ', "_"),
        }
    }

    /// A value of `shape` the reader takes, written at `path`; every
    /// alternative of a `OneOf` at the top, the first one anywhere deeper.
    /// `poisoned` is the path of a dictionary to give a key that isn't one.
    fn samples(path: &str, shape: &ConfigShape, poisoned: Option<&str>) -> Vec<String> {
        match shape {
            ConfigShape::OneOf(alternatives) => alternatives
                .iter()
                .flat_map(|a| samples(path, a, poisoned))
                .collect(),
            ConfigShape::Integer { values } => {
                vec![values.first().copied().unwrap_or(1).to_string()]
            }
            ConfigShape::String => vec![match path {
                "id" => format!("\"{}\"", "0123456789abcdef".repeat(2)),
                "encodings.<name>@<number>.profile" => "\"profiles/packed@1.json\"".into(),
                _ => "\"text\"".into(),
            }],
            // `comline@1` takes no parameters, which the dictionary writes.
            ConfigShape::Choice { .. } if path == "handshake.name" => vec!["\"comline@2\"".into()],
            ConfigShape::Choice { values, .. } => vec![format!("\"{}\"", values[0])],
            ConfigShape::List(item) => {
                vec![format!("[{}]", samples(path, item, poisoned).remove(0))]
            }
            ConfigShape::Dictionary(keys) => {
                let mut entries: Vec<String> = keys
                    .iter()
                    .map(|k| {
                        let value =
                            samples(&format!("{path}.{}", k.key), &k.value, poisoned).remove(0);
                        format!("{} = {value}", k.key)
                    })
                    .collect();
                if poisoned == Some(path) {
                    entries.push("zz_not_a_key = \"text\"".into());
                }
                vec![format!("{{ {} }}", entries.join("\n"))]
            }
            ConfigShape::Map(entry) => {
                samples(&format!("{path}.{}", entry.key), &entry.value, poisoned)
                    .into_iter()
                    .map(|value| format!("{{ {} = {value} }}", example_name(&entry.key)))
                    .collect()
            }
            ConfigShape::Settings => vec!["{ mode = replace }".into()],
        }
    }

    /// What the reader makes of `key = value`, as the package manifest's
    /// first assignment: the encodings package's origin, which has the
    /// official profiles `encodings` reads.
    fn read(key: &str, value: &str) -> Result<(), String> {
        let source = format!("congregation sample\nspecification_version = 1\n{key} = {value}\n");
        let mut context =
            ProjectInterpreter::from_config_source(&source).map_err(|e| e.to_string())?;
        context.origin = Origin::Official;
        let assignment = context
            .config
            .assignments
            .iter()
            .find(|a| a.key.text() == key)
            .unwrap_or_else(|| panic!("`{key}` didn't parse in:\n{source}"));
        freezing::interpret_assignment(&context, assignment, None)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    #[test]
    fn the_reader_takes_every_key_the_table_lists() {
        for key in keys() {
            for value in samples(&key.key, &key.value, None) {
                if let Err(error) = read(&key.key, &value) {
                    panic!(
                        "the table says `{} = {value}` is valid, the reader says: {error}",
                        key.key
                    );
                }
            }
        }
    }

    /// Every dictionary the table closes: a key it doesn't list is refused.
    /// Not `dependencies`' entries, whose reader skips what it doesn't know,
    /// nor `unverified`, whose entries are judged where the guarantees are
    /// known; the reader's keys are held to the table below instead.
    #[test]
    fn the_reader_refuses_a_key_the_table_does_not_list() {
        fn dictionaries(path: &str, shape: &ConfigShape, found: &mut Vec<String>) {
            match shape {
                ConfigShape::Dictionary(keys) => {
                    found.push(path.to_string());
                    for key in keys {
                        dictionaries(&format!("{path}.{}", key.key), &key.value, found);
                    }
                }
                ConfigShape::Map(entry) => {
                    dictionaries(&format!("{path}.{}", entry.key), &entry.value, found)
                }
                ConfigShape::OneOf(alternatives) => alternatives
                    .iter()
                    .for_each(|a| dictionaries(path, a, found)),
                ConfigShape::List(item) => dictionaries(path, item, found),
                _ => {}
            }
        }

        let mut closed = 0;
        for key in keys() {
            let mut found = vec![];
            dictionaries(&key.key, &key.value, &mut found);
            for path in found {
                if path.starts_with("dependencies.") || path == "unverified" {
                    continue;
                }
                closed += 1;
                let value = samples(&key.key, &key.value, Some(&path))
                    .into_iter()
                    .find(|v| v.contains("zz_not_a_key"))
                    .unwrap_or_else(|| panic!("no sample reaches `{path}`"));
                assert!(
                    read(&key.key, &value).is_err(),
                    "the reader takes a key the table doesn't list in `{path}`:\n{} = {value}",
                    key.key
                );
            }
        }
        assert!(closed >= 7, "only {closed} dictionaries were checked");
    }

    /// Every key name in the reader's source, which is a `match` on them: the
    /// string literals in a pattern, before its `=>`. A key added to the
    /// reader without the table fails this, as a key the table lacks fails
    /// [`the_reader_takes_every_key_the_table_lists`].
    fn names_in_patterns(source: &str) -> Vec<String> {
        // Drop `/* ... */`, where an arm is commented out.
        let mut code = String::new();
        let mut rest = source;
        while let Some(start) = rest.find("/*") {
            code.push_str(&rest[..start]);
            rest = &rest[start..];
            rest = rest.find("*/").map_or("", |end| &rest[end + 2..]);
        }
        code.push_str(rest);

        let mut names = vec![];
        for line in code.lines() {
            let line = line.trim_start();
            if line.starts_with("//") {
                continue;
            }
            let Some(arrow) = line.find("=>") else {
                continue;
            };
            let mut pattern = &line[..arrow];
            while let Some(open) = pattern.find('"') {
                let Some(len) = pattern[open + 1..].find('"') else {
                    break;
                };
                names.push(pattern[open + 1..open + 1 + len].to_string());
                pattern = &pattern[open + 2 + len..];
            }
        }
        names
    }

    #[test]
    fn the_table_lists_every_key_the_reader_matches() {
        fn all(shape: &ConfigShape, names: &mut Vec<String>) {
            match shape {
                ConfigShape::Dictionary(keys) => {
                    for key in keys {
                        names.push(key.key.clone());
                        all(&key.value, names);
                    }
                }
                ConfigShape::Map(entry) => all(&entry.value, names),
                ConfigShape::OneOf(alternatives) => alternatives.iter().for_each(|a| all(a, names)),
                ConfigShape::List(item) => all(item, names),
                ConfigShape::Choice { values, .. } => names.extend(values.iter().cloned()),
                _ => {}
            }
        }
        let mut listed = vec![];
        for key in keys() {
            listed.push(key.key.clone());
            all(&key.value, &mut listed);
        }

        let readers = [
            (
                "ir/interpreter/freezing.rs",
                include_str!("ir/interpreter/freezing.rs"),
            ),
            ("dependency.rs", include_str!("dependency.rs")),
        ];
        for (file, source) in readers {
            let names = names_in_patterns(source);
            assert!(
                names.len() >= 5,
                "{file}: found {names:?}, the scan has stopped working"
            );
            for name in names {
                assert!(
                    listed.contains(&name),
                    "{file} matches `{name}`, which `package::config::keys` doesn't list: add it there"
                );
            }
        }
    }
}
