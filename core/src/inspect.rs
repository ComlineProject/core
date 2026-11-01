//! What a package's protocols resolve to on the wire, and what they keep:
//! the model behind `comline inspect` and the language server's hover on a
//! protocol (cli#83).
//!
//! For each protocol, [`protocols`] reports the format, framing and
//! handshake it uses, their parameters and where each came from
//! ([`Source`]), then every [`Guarantee`]: held or given up, why
//! ([`unverified::causes`]), the `@unverified` reason, and whether it's
//! been accepted ([`Acceptance`]).
//!
//! It reads only what compiling already produced; nothing is written.
//!
//! **Provenance** isn't in the frozen IR: compiling applies the package's
//! `wire.framing` and `handshake` to every protocol that names none, so a
//! frozen `framing` property doesn't say who wrote it. It's read from the
//! protocol's declaration instead (the schema's syntax tree, which a
//! compiled context keeps, a dependency's included): an `@framing` or
//! `@handshake` written there is the protocol's own; anything else is the
//! package's, if its config names one, and core's otherwise.
//!
//! **Acceptance** is knowable one way only. A package lists, under each
//! dependency in its `config.idp`, what it accepts of that dependency's
//! `@unverified` protocols (`accept_unverified`), so for a dependency's
//! protocol the inspected package is the consumer, and it either accepts
//! it or doesn't. A package can't see its own consumers, so for its own
//! protocols acceptance is [`Acceptance::Unknown`].
//!
//! Compatibility facts (a protocol's line, version or shape hash) aren't
//! here: they don't exist yet (core#119).

// Standard Uses
use std::collections::BTreeMap;

// Crate Uses
use crate::package::config::ir::context::ProjectContext;
use crate::package::config::ir::frozen::FrozenUnit as ConfigUnit;
use crate::schema::idl::annotations::{DEFAULT_FORMAT, DEFAULT_FRAMING, DEFAULT_HANDSHAKE, FORMAT_JSON, FRAMING_JSONRPC};
use crate::schema::idl::grammar::Declaration;
use crate::schema::ir::frozen::unit::{ConfigValue, FrozenUnit, Guarantee};
use crate::schema::ir::unverified::{self, Encodings};

// External Uses
use schemars::JsonSchema;
use serde_derive::{Deserialize, Serialize};


/// One protocol, as [`protocols`] reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProtocolSummary {
    /// The package that declares it: the inspected package's name, or the
    /// dependency's, as the inspected package names it.
    pub package: String,
    /// Whether it's a dependency's protocol rather than the package's own.
    pub dependency: bool,
    /// The schema it's in (`api`, or `billing::api` for a dependency's).
    pub schema: String,
    pub name: String,
    pub format: Choice,
    pub framing: Choice,
    pub handshake: Choice,
    /// Whether it's marked `@unverified`.
    pub unverified: bool,
    /// Every guarantee, in [`Guarantee::ALL`] order.
    pub guarantees: Vec<GuaranteeRow>,
}

/// A resolved encoding: its name, its parameters and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Choice {
    /// The name, canonical (`datagram@2`, `jsonrpc`, `comline@1`).
    pub value: String,
    /// Its parameters (`@framing`'s dictionary form, less `name`), each
    /// value as written.
    pub params: BTreeMap<String, String>,
    pub source: Source,
}

/// Where a [`Choice`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The protocol's own `@framing` or `@handshake`.
    Annotation,
    /// The package's `wire` or `handshake` in its `config.idp`.
    PackageDefault,
    /// Core's default (`packed@1`, `datagram@2`, `comline@1`).
    CoreDefault,
    /// The format JSON-RPC implies: JSON, when the package names no
    /// format.
    Framing,
}

impl Source {
    /// In prose: `@framing`, `config.idp`, `default`, `framing`.
    pub fn describe(self, key: &str) -> String {
        match self {
            Source::Annotation => format!("@{key}"),
            Source::PackageDefault => "config.idp".to_string(),
            Source::CoreDefault => "default".to_string(),
            Source::Framing => "implied by the framing".to_string(),
        }
    }
}

/// One guarantee of one protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GuaranteeRow {
    #[serde(with = "guarantee_name")]
    #[schemars(with = "String")]
    pub guarantee: Guarantee,
    pub held: bool,
    /// Why it's given up, one entry per cause ([`unverified::causes`]).
    /// Empty when held.
    pub causes: Vec<String>,
    /// The `@unverified` reason, when it's given up and the marker carries
    /// one.
    pub reason: Option<String>,
    pub acceptance: Acceptance,
}

/// Whether a given-up guarantee has been accepted by a consumer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "state", content = "by")]
pub enum Acceptance {
    /// It's held: there's nothing to accept.
    NotNeeded,
    /// The package's own protocol: its consumers accept it in their own
    /// `config.idp`, which a package can't see.
    Unknown,
    /// A dependency's protocol, accepted by the inspected package (named).
    Accepted(String),
    /// A dependency's protocol the inspected package doesn't accept (the
    /// build refuses it).
    NotAccepted,
}

impl Acceptance {
    /// In prose, short: `-`, `unknown`, `accepted by app`, `not accepted`.
    pub fn describe(&self) -> String {
        match self {
            Acceptance::NotNeeded => "-".to_string(),
            Acceptance::Unknown => "unknown".to_string(),
            Acceptance::Accepted(by) => format!("accepted by {by}"),
            Acceptance::NotAccepted => "not accepted".to_string(),
        }
    }
}

/// [`Guarantee`] as its written name (`schema_agreement`).
mod guarantee_name {
    use crate::schema::ir::frozen::unit::Guarantee;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(guarantee: &Guarantee, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(guarantee.name())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Guarantee, D::Error> {
        let name = String::deserialize(d)?;
        Guarantee::from_name(&name)
            .ok_or_else(|| serde::de::Error::custom(format!("`{name}` isn't a guarantee")))
    }
}

/// Every protocol `context` compiled, its own then its dependencies', each
/// in schema and declaration order. Std's schemas are skipped.
pub fn protocols(context: &ProjectContext) -> Vec<ProtocolSummary> {
    let own_config = context.config_frozen.as_deref().unwrap_or(&[]);
    let own_name = context.config.name.value.to_string();
    let accepts = accepted(context);
    let all = Encodings::of(context);

    let mut own = vec![];
    let mut theirs = vec![];
    for schema in &context.schema_contexts {
        let schema = schema.borrow();
        if schema.namespace.first().map(String::as_str) == Some("std") {
            continue;
        }
        // A dependency's schemas are prefixed with its name.
        let dependency = match schema.namespace.as_slice() {
            [first, _, ..] => context
                .dependency_configs
                .iter()
                .find(|(name, _)| name == first)
                .map(|(name, units)| (name.as_str(), units.as_slice())),
            _ => None,
        };
        let config = dependency.map(|(_, units)| units).unwrap_or(own_config);
        let (format, framing) = crate::package::config::ir::frozen::wire(config);
        let encodings = Encodings { format, framing, ..all.clone() };

        let frozen = schema.frozen_schema.borrow();
        let Some(units) = frozen.as_ref() else { continue };
        for unit in units {
            let FrozenUnit::Protocol { name, parameters, .. } = unit else { continue };
            let written = written_keys(&schema.declarations, name);
            let summary = summarize(Protocol {
                package: dependency.map(|(n, _)| n).unwrap_or(&own_name),
                dependency: dependency.is_some(),
                schema: schema.namespace.join("::"),
                name,
                parameters,
                written: &written,
                config,
                encodings: &encodings,
                accepted: dependency.map(|(dep, _)| {
                    (own_name.as_str(), accepts.get(dep).and_then(|protocols| protocols.get(name)))
                }),
            });
            if dependency.is_some() {
                theirs.push(summary);
            } else {
                own.push(summary);
            }
        }
    }
    own.extend(theirs);
    own
}

/// The protocol `name` in the schema `schema` (`api`, `billing::api`), if
/// `context` compiled it.
pub fn protocol(context: &ProjectContext, schema: &str, name: &str) -> Option<ProtocolSummary> {
    protocols(context).into_iter().find(|p| p.schema == schema && p.name == name)
}

struct Protocol<'a> {
    package: &'a str,
    dependency: bool,
    schema: String,
    name: &'a str,
    parameters: &'a [FrozenUnit],
    /// The annotation keys its declaration writes.
    written: &'a [String],
    /// Its package's frozen config.
    config: &'a [ConfigUnit],
    encodings: &'a Encodings<'a>,
    /// For a dependency's protocol: the consumer, and what it accepts.
    accepted: Option<(&'a str, Option<&'a Vec<Guarantee>>)>,
}

fn summarize(p: Protocol) -> ProtocolSummary {
    let property = |key: &str| {
        p.parameters.iter().find_map(|u| match u {
            FrozenUnit::Property { name, value } if name == key => Some(value),
            _ => None,
        })
    };
    let (wire_format, wire_framing) = crate::package::config::ir::frozen::wire(p.config);
    let package_handshake = p.config.iter().any(|u| matches!(u, ConfigUnit::Handshake(_)));

    let framing = match property("framing") {
        Some(value) => {
            let (name, params) = split(value);
            let name = crate::schema::idl::annotations::canonical_value("framing", &name)
                .map(str::to_string)
                .unwrap_or(name);
            let source = if p.written.iter().any(|k| k == "framing") {
                Source::Annotation
            } else {
                Source::PackageDefault
            };
            Choice { value: name, params, source }
        }
        None => match wire_framing {
            Some(framing) => Choice { value: framing.to_string(), params: BTreeMap::new(), source: Source::PackageDefault },
            None => Choice { value: DEFAULT_FRAMING.to_string(), params: BTreeMap::new(), source: Source::CoreDefault },
        },
    };

    let format = match wire_format {
        Some(format) => Choice { value: format.to_string(), params: BTreeMap::new(), source: Source::PackageDefault },
        None if framing.value == FRAMING_JSONRPC => {
            Choice { value: FORMAT_JSON.to_string(), params: BTreeMap::new(), source: Source::Framing }
        }
        None => Choice { value: DEFAULT_FORMAT.to_string(), params: BTreeMap::new(), source: Source::CoreDefault },
    };

    let handshake = {
        let value = unverified::handshake(p.parameters).to_string();
        let source = if p.written.iter().any(|k| k == "handshake") {
            Source::Annotation
        } else if package_handshake {
            Source::PackageDefault
        } else {
            debug_assert_eq!(value, DEFAULT_HANDSHAKE);
            Source::CoreDefault
        };
        Choice { value, params: BTreeMap::new(), source }
    };

    let causes = unverified::causes(p.parameters, p.encodings);
    let reason = unverified::reason(p.parameters).map(str::to_string);
    let guarantees = Guarantee::ALL
        .into_iter()
        .map(|guarantee| {
            let why: Vec<String> =
                causes.iter().filter(|(g, _)| *g == guarantee).map(|(_, c)| c.clone()).collect();
            let held = why.is_empty();
            let acceptance = if held {
                Acceptance::NotNeeded
            } else {
                match p.accepted {
                    None => Acceptance::Unknown,
                    Some((consumer, Some(set))) if set.contains(&guarantee) => {
                        Acceptance::Accepted(consumer.to_string())
                    }
                    Some(_) => Acceptance::NotAccepted,
                }
            };
            GuaranteeRow {
                guarantee,
                held,
                causes: why,
                reason: if held { None } else { reason.clone() },
                acceptance,
            }
        })
        .collect();

    ProtocolSummary {
        package: p.package.to_string(),
        dependency: p.dependency,
        schema: p.schema,
        name: p.name.to_string(),
        format,
        framing,
        handshake,
        unverified: unverified::marker(p.parameters).is_some(),
        guarantees,
    }
}

/// A `@framing` value's name and its other entries, each as written.
pub(crate) fn split(value: &ConfigValue) -> (String, BTreeMap<String, String>) {
    match value {
        ConfigValue::Dict(entries) => {
            let mut name = String::new();
            let mut params = BTreeMap::new();
            for (key, value) in entries {
                if key == "name" {
                    name = render(value);
                } else {
                    params.insert(key.clone(), render(value));
                }
            }
            (name, params)
        }
        other => (render(other), BTreeMap::new()),
    }
}

/// A parameter value, as written, less a string's quotes.
fn render(value: &ConfigValue) -> String {
    match value {
        ConfigValue::Marker => String::new(),
        ConfigValue::Bool(b) => b.to_string(),
        ConfigValue::Integer(i) => i.to_string(),
        ConfigValue::Str(s) | ConfigValue::Name(s) | ConfigValue::Path(s) | ConfigValue::FString(s) => s.clone(),
        ConfigValue::Dict(entries) => {
            let inner: Vec<String> = entries.iter().map(|(k, v)| format!("{k} = {}", render(v))).collect();
            format!("{{ {} }}", inner.join(", "))
        }
    }
}

/// The annotation keys the declaration of protocol `name` writes.
fn written_keys(declarations: &[rust_sitter::Spanned<Declaration>], name: &str) -> Vec<String> {
    declarations
        .iter()
        .find_map(|d| match &d.value {
            Declaration::Protocol(protocol) if protocol.name() == name => {
                Some(protocol.annotations().iter().map(|a| a.key.to_string()).collect())
            }
            _ => None,
        })
        .unwrap_or_default()
}

/// What the package accepts of each dependency's protocols:
/// dependency -> protocol -> guarantees.
fn accepted(context: &ProjectContext) -> BTreeMap<String, BTreeMap<String, Vec<Guarantee>>> {
    crate::package::config::dependency::DependencyConfig::parse_dependencies(&context.config.assignments)
        .unwrap_or_default()
        .into_iter()
        .map(|(name, dep)| (name, dep.accept_unverified.into_iter().collect()))
        .collect()
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn package(dir: &Path, config: &str, schema: &str) {
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let name = dir.file_name().unwrap().to_str().unwrap();
        std::fs::write(
            dir.join("config.idp"),
            format!("congregation {name}\nspecification_version = 1\n{config}"),
        )
        .unwrap();
        std::fs::write(dir.join("src/api.ids"), schema).unwrap();
    }

    fn row(summary: &ProtocolSummary, guarantee: Guarantee) -> &GuaranteeRow {
        summary.guarantees.iter().find(|r| r.guarantee == guarantee).unwrap()
    }

    #[test]
    fn core_defaults_hold_everything() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("app");
        package(&app, "", "protocol Plain {\n    function ping();\n}\n");
        let context = crate::package::build::compile_package(&app).unwrap();

        let [plain] = protocols(&context).try_into().unwrap();
        assert_eq!((plain.package.as_str(), plain.schema.as_str(), plain.name.as_str()), ("app", "api", "Plain"));
        assert!(!plain.dependency && !plain.unverified);
        for (choice, value) in [(&plain.format, DEFAULT_FORMAT), (&plain.framing, DEFAULT_FRAMING), (&plain.handshake, DEFAULT_HANDSHAKE)] {
            assert_eq!((choice.value.as_str(), choice.source), (value, Source::CoreDefault));
            assert!(choice.params.is_empty());
        }
        assert_eq!(plain.guarantees.len(), Guarantee::ALL.len());
        for row in &plain.guarantees {
            assert!(row.held && row.causes.is_empty() && row.reason.is_none(), "{row:?}");
            assert_eq!(row.acceptance, Acceptance::NotNeeded);
        }
    }

    #[test]
    fn each_choice_says_where_it_came_from() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("app");
        package(
            &app,
            "wire = { framing = \"datagram@1\" }\nhandshake = \"comline@1\"\n\
             settings = { annotations = { unverified = { allowed = true } framing = { allowed = true } } }\n",
            "protocol Defaults {\n    function ping();\n}\n\n\
             @framing = { name = \"jsonrpc\", method_names = \"qualified\" }\n\
             @handshake = \"none\"\n@unverified = \"the 2009 gateway\"\n\
             protocol Own {\n    function ping();\n}\n",
        );
        let context = crate::package::build::compile_package(&app).unwrap();
        let [defaults, own] = protocols(&context).try_into().unwrap();

        assert_eq!((defaults.framing.value.as_str(), defaults.framing.source), ("datagram", Source::PackageDefault));
        assert_eq!((defaults.handshake.value.as_str(), defaults.handshake.source), ("comline@1", Source::PackageDefault));
        assert_eq!((defaults.format.value.as_str(), defaults.format.source), (DEFAULT_FORMAT, Source::CoreDefault));
        assert!(defaults.guarantees.iter().all(|r| r.held));

        assert_eq!((own.framing.value.as_str(), own.framing.source), ("jsonrpc", Source::Annotation));
        assert_eq!(own.framing.params, BTreeMap::from([("method_names".to_string(), "qualified".to_string())]));
        assert_eq!((own.handshake.value.as_str(), own.handshake.source), ("none", Source::Annotation));
        assert_eq!((own.format.value.as_str(), own.format.source), (FORMAT_JSON, Source::Framing));
        assert!(own.unverified);
        for guarantee in [Guarantee::SchemaAgreement, Guarantee::EncodingAgreement] {
            let row = row(&own, guarantee);
            assert!(!row.held);
            assert_eq!(row.causes, ["handshake `none`"]);
            assert_eq!(row.reason.as_deref(), Some("the 2009 gateway"));
            assert_eq!(row.acceptance, Acceptance::Unknown);
        }
        for guarantee in [Guarantee::EncodingConformance, Guarantee::VersionHonesty] {
            assert!(row(&own, guarantee).held);
            assert_eq!(row(&own, guarantee).reason, None);
        }

        // `--json`: guarantees by their written names, the rest snake_case.
        let json = serde_json::to_value(&own).unwrap();
        assert_eq!(json["guarantees"][0]["guarantee"], "schema_agreement");
        assert_eq!(json["guarantees"][0]["acceptance"], serde_json::json!({ "state": "unknown" }));
        assert_eq!(json["format"]["source"], "framing");
        let back: ProtocolSummary = serde_json::from_value(json).unwrap();
        assert_eq!(back, own);
    }

    #[test]
    fn a_custom_encoding_without_vectors_gives_up_conformance() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("app");
        std::fs::create_dir_all(app.join("profiles")).unwrap();
        std::fs::write(
            app.join("profiles/f.json"),
            r#"{ "name": "com.acme.f@1", "kind": "framing",
                 "facts": { "calls": { "value": "by_name" }, "errors": { "value": "by_name" } } }"#,
        )
        .unwrap();
        package(
            &app,
            "encodings = {\n    \"com.acme.f@1\" = { kind = \"framing\" profile = \"profiles/f.json\" }\n}\n\
             settings = { annotations = { unverified = { allowed = true } framing = { allowed = true } } }\n",
            "@framing = \"com.acme.f@1\"\n@unverified = \"no vectors yet\"\nprotocol P {\n    function ping();\n}\n",
        );
        let context = crate::package::build::compile_package(&app).unwrap();
        let [p] = protocols(&context).try_into().unwrap();
        let row = row(&p, Guarantee::EncodingConformance);
        assert!(!row.held);
        assert_eq!(row.causes, ["framing `com.acme.f@1` ships no conformance vectors"]);
        assert_eq!(row.reason.as_deref(), Some("no vectors yet"));
        assert!(row_held(&p, Guarantee::SchemaAgreement));
    }

    fn row_held(summary: &ProtocolSummary, guarantee: Guarantee) -> bool {
        row(summary, guarantee).held
    }

    #[cfg(feature = "deps")]
    #[test]
    fn a_dependencys_protocol_is_accepted_by_the_consumer() {
        let tmp = tempfile::tempdir().unwrap();
        let billing = tmp.path().join("billing");
        package(
            &billing,
            "handshake = \"none\"\nsettings = { annotations = { unverified = { allowed = true } } }\n",
            "@unverified = \"checked out of band\"\nprotocol Billing {\n    function charge();\n}\n",
        );
        let app = tmp.path().join("app");
        package(
            &app,
            "dependencies = {\n    billing = {\n        path = \"../billing\"\n        \
             accept_unverified = { Billing = [\"schema_agreement\", \"encoding_agreement\"] }\n    }\n}\n",
            "protocol Mine {\n    function ping();\n}\n",
        );
        let context = crate::package::build::compile_package(&app).unwrap();
        let [mine, billing] = protocols(&context).try_into().unwrap();
        assert!(!mine.dependency);
        assert_eq!((billing.package.as_str(), billing.schema.as_str()), ("billing", "billing::api"));
        assert!(billing.dependency);
        // The dependency's own default, not the consumer's.
        assert_eq!((billing.handshake.value.as_str(), billing.handshake.source), ("none", Source::PackageDefault));
        for guarantee in [Guarantee::SchemaAgreement, Guarantee::EncodingAgreement] {
            let row = row(&billing, guarantee);
            assert!(!row.held);
            assert_eq!(row.reason.as_deref(), Some("checked out of band"));
            assert_eq!(row.acceptance, Acceptance::Accepted("app".to_string()));
        }
        assert_eq!(protocol(&context, "billing::api", "Billing"), Some(billing));
        assert_eq!(protocol(&context, "api", "Billing"), None);
    }
}
