//! Named connection sets: `@connection = admin`, or a list
//! (`@connection = [admin, public]`), puts a protocol in a set that runs on a
//! connection of its own (core#300, core#301). This reads the annotation from
//! the parsed schema and checks it, the package's `connections` table and the
//! encodings of every set, collecting every error.
//!
//! **Not frozen.** `@connection` is read here, from the source, and
//! `annotation_units` leaves it out of the frozen IR, so a package hashes the
//! same with the annotation as without it. Set membership enters the IR, with
//! the version rules that go with it, in core#302. The permission to write it
//! (`annotations.connection.allowed`) is still the settings gate's:
//! [`with_gate_units`] hands it the annotation without freezing it.
//!
//! **What a set needs of its encodings is read from their facts**, never from
//! their names. A handshake has to state `schema_comparison = per_protocol`
//! (a handshake that doesn't state it is `whole_schema`, the catalog's default
//! for the fact, and one with no profile at all, `none`, sends no Hello that
//! could enforce anything), and a framing `protocol = named`,
//! `directions = both` and `calls = by_id`: its frames say which protocol a
//! call is for, and a protocol's number doesn't depend on its position. Today
//! that is `comline@2` with `datagram@3`; a handshake or framing that states
//! the same facts is as good, and one that doesn't isn't, whatever it is
//! called.
//!
//! **Which protocols run on a connection** is read the same way: one whose
//! framing says whether a frame is a request or a reply (`directions = both`)
//! can share a transport with others; lockstep `datagram@1` can't.

use std::collections::BTreeSet;

use comline_catalog::fact;

use crate::config_value;
use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::encodings::{official, EncodingKind};
use crate::package::config::ir::interpreter::freezing::SetEncodings;
use crate::schema::idl::annotations::{self, FramingValue, HandshakeValue, CONNECTION};
use crate::schema::idl::grammar::{Annotation, AnnotationValue, Expression};
use crate::schema::ir::compiler::interpreter::incremental::config_value_of;
use crate::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};
use crate::schema::ir::package_wire::PackageWire;
use crate::schema::ir::unverified::{framing_fact, Encodings};

const UNASSIGNED: DiagnosticCode = DiagnosticCode::error("connection::unassigned");
const NOT_ON_A_CONNECTION: DiagnosticCode =
    DiagnosticCode::error("connection::not-on-a-connection");
const BAD_SET_NAME: DiagnosticCode = DiagnosticCode::error("connection::bad-set-name");
const UNSUPPORTED_ENCODING: DiagnosticCode =
    DiagnosticCode::error("connection::unsupported-encoding");
const ENCODING_MISMATCH: DiagnosticCode = DiagnosticCode::error("connection::encoding-mismatch");
const UNKNOWN_SET: DiagnosticCode = DiagnosticCode::error("connection::unknown-set");
const DUPLICATE_SET: DiagnosticCode = DiagnosticCode::error("connection::duplicate-set");
const EMPTY_LIST: DiagnosticCode = DiagnosticCode::error("connection::empty-list");
const ANNOTATION_VALUE_MISMATCH: DiagnosticCode =
    DiagnosticCode::error("validation::annotation-value-mismatch");

/// What a framing has to state for a set's protocols to share a connection
/// and be numbered by what they are, not where they stand: the fact, the value
/// and why.
const FRAMING_NEEDS: &[(&str, &str, &str)] = &[
    (
        fact::PROTOCOL,
        "named",
        "a call has to say which protocol it is for",
    ),
    (
        fact::DIRECTIONS,
        "both",
        "a frame has to say whether it is a request or a reply",
    ),
    (
        fact::CALLS,
        "by_id",
        "a protocol's number must not depend on its position",
    ),
];

/// Words a set's name can't be because a target language has them as keywords:
/// a set is a module of that name in the generated code. Rust's, strict and
/// reserved, then TypeScript's reserved and strict-mode words and the names of
/// its primitive types. Only lowercase words are here: a set's name is
/// lowercase `snake_case`, so the others can't clash.
const KEYWORDS: &[(&str, &[&str])] = &[
    (
        "Rust",
        &[
            "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
            "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
            "move", "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait",
            "true", "type", "unsafe", "use", "where", "while", "abstract", "become", "box", "do",
            "final", "gen", "macro", "override", "priv", "try", "typeof", "unsized", "virtual",
            "yield",
        ],
    ),
    (
        "TypeScript",
        &[
            "any",
            "as",
            "async",
            "await",
            "bigint",
            "boolean",
            "break",
            "case",
            "catch",
            "class",
            "const",
            "continue",
            "debugger",
            "declare",
            "default",
            "delete",
            "do",
            "else",
            "enum",
            "export",
            "extends",
            "false",
            "finally",
            "for",
            "from",
            "function",
            "get",
            "if",
            "implements",
            "import",
            "in",
            "infer",
            "instanceof",
            "interface",
            "is",
            "keyof",
            "let",
            "module",
            "namespace",
            "never",
            "new",
            "null",
            "number",
            "object",
            "of",
            "package",
            "private",
            "protected",
            "public",
            "readonly",
            "require",
            "return",
            "set",
            "static",
            "string",
            "super",
            "switch",
            "symbol",
            "this",
            "throw",
            "true",
            "try",
            "type",
            "typeof",
            "undefined",
            "unique",
            "unknown",
            "var",
            "void",
            "while",
            "with",
            "yield",
        ],
    ),
];

/// What a set's generated module already holds, in `snake_case`: the names
/// of the items of `connection::<set>` (`Services`, `ClientServices`,
/// `accept`, `serve`, `connect`, `Connection`, `Framing`, `Format`), so a set
/// with one of them would collide with it.
const MODULE_ITEMS: &[&str] = &[
    "services",
    "client_services",
    "accept",
    "serve",
    "connect",
    "connection",
    "framing",
    "format",
];

/// A set's name as written in a `@connection`, where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetName {
    pub text: String,
    pub span: (usize, usize),
}

/// What a protocol's `@connection` says.
#[derive(Debug, Clone)]
pub enum Reading {
    /// The sets it names, in the order written.
    Sets(Vec<SetName>),
    /// `@connection = []`: it names no set, which is its own error. Neither
    /// a protocol left out nor in a set.
    Empty(Diagnostic),
    /// A value that isn't a name or a list of names. Its error is this; the
    /// protocol is neither left out nor in a set.
    Invalid(Diagnostic),
}

/// One protocol of the package, as the checks read it.
#[derive(Debug, Clone)]
pub struct Protocol {
    /// Which of the package's schemas it is in, for the caller to render a
    /// finding against.
    pub schema: usize,
    pub name: String,
    pub span: (usize, usize),
    /// `@connection`, where the protocol writes one.
    pub connection: Option<Reading>,
    /// What the protocol writes itself of its framing and handshake: its
    /// frozen parameters before the package's are added.
    pub parameters: Vec<FrozenUnit>,
}

impl Protocol {
    /// A protocol with these `annotations`, in schema `schema`.
    pub fn read(
        schema: usize,
        name: &str,
        span: (usize, usize),
        annotations: &[&Annotation],
        parameters: Vec<FrozenUnit>,
    ) -> Protocol {
        let connection = annotations
            .iter()
            .find(|a| a.key() == CONNECTION)
            .map(|annotation| read_annotation(annotation, name, span));
        Protocol {
            schema,
            name: name.to_string(),
            span,
            connection,
            parameters,
        }
    }
}

/// A finding, with the schema it is about: `None` for one about `config.idp`.
#[derive(Debug, Clone)]
pub struct Finding {
    pub schema: Option<usize>,
    pub diagnostic: Diagnostic,
}

/// What the checks read of the package besides its protocols.
pub struct Package<'a> {
    /// The package's framing and handshake, which a set takes where it states
    /// none.
    pub wire: &'a PackageWire,
    /// `connections` in `config.idp`: each set that states its own.
    pub table: &'a [(String, SetEncodings)],
    /// The encodings a name can be declared by.
    pub encodings: &'a Encodings<'a>,
}

/// `@connection`'s value: a bare name, or a list of bare names.
fn read_annotation(annotation: &Annotation, protocol: &str, span: (usize, usize)) -> Reading {
    let context = format!("protocol '{protocol}'");
    let mismatch = |message: String, help: String| {
        Reading::Invalid(
            Diagnostic::new(ANNOTATION_VALUE_MISMATCH, message)
                .with_context(context.clone())
                .with_help(help)
                .with_span(span),
        )
    };
    let takes = annotations::lookup(CONNECTION)
        .map(|info| config_value::describe(&info.value))
        .unwrap_or_default();
    let wrong = |found: config_value::ValueForm<'_>| {
        format!("'@{CONNECTION}' must be {takes}, got {found}")
    };
    let help = format!(
        "a set is named by a bare identifier, never a string: `@{CONNECTION} = admin`, or \
         `@{CONNECTION} = [admin, public]` for a protocol in several sets"
    );

    let Some(assignment) = &annotation.assignment else {
        let found = if annotation.call.is_some() {
            config_value::ValueForm::Dict
        } else {
            config_value::ValueForm::Marker
        };
        return mismatch(wrong(found), help);
    };
    match &assignment.value {
        AnnotationValue::Scalar(Expression::Identifier(name)) => Reading::Sets(vec![SetName {
            text: name.text.clone(),
            span,
        }]),
        AnnotationValue::Scalar(expression) => {
            let value = config_value_of(expression);
            let help = match expression {
                // The one mistake worth a fix: the name, quoted.
                Expression::String(quoted) => format!(
                    "drop the quotes: `@{CONNECTION} = {}`. {help}",
                    quoted.value
                ),
                _ => help,
            };
            mismatch(wrong(value.form()), help)
        }
        AnnotationValue::Dict(_) => mismatch(wrong(config_value::ValueForm::Dict), help),
        AnnotationValue::List(list) if list.items.is_empty() => Reading::Empty(
            Diagnostic::new(EMPTY_LIST, format!("'@{CONNECTION} = []' names no set"))
                .with_context(context.clone())
                .with_help(format!(
                    "name at least one set, `@{CONNECTION} = admin`, or remove `@{CONNECTION}` \
                     to leave the protocol out of the sets, which a package with sets can't \
                     do for a protocol that runs on a connection"
                ))
                .with_span(span),
        ),
        AnnotationValue::List(list) => {
            let mut names = vec![];
            for call in &list.items {
                if call.call.is_some() {
                    return mismatch(
                        format!(
                            "'@{CONNECTION}' must be {takes}, got `{}(...)`, a call",
                            call.name.text
                        ),
                        help,
                    );
                }
                names.push(SetName {
                    text: call.name.text.clone(),
                    span: call.name.span,
                });
            }
            Reading::Sets(names)
        }
    }
}

/// `units`, the frozen units of schema number `schema`, with `@connection` put
/// back on the protocols that write it, for
/// the settings gate (`annotations.connection.allowed`) and nothing else: the
/// frozen schema never has it.
pub fn with_gate_units(
    units: &[FrozenUnit],
    protocols: &[Protocol],
    schema: usize,
) -> Vec<FrozenUnit> {
    let mut gated = units.to_vec();
    for unit in &mut gated {
        if let FrozenUnit::Protocol {
            name, parameters, ..
        } = unit
        {
            if protocols
                .iter()
                .any(|p| p.schema == schema && p.name == *name && p.connection.is_some())
            {
                parameters.push(FrozenUnit::Property {
                    name: CONNECTION.to_string(),
                    value: ConfigValue::Marker,
                });
            }
        }
    }
    gated
}

/// A set's encodings as the checks compare them: its framing and handshake,
/// and where each came from.
struct Effective {
    framing: String,
    framing_from: Source,
    handshake: HandshakeValue,
    handshake_from: Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// Stated in `connections`.
    Set,
    /// The package's `wire` or `handshake`, or its default.
    Package,
}

impl Source {
    fn whose(self) -> &'static str {
        match self {
            Source::Set => "the set's own, in `connections`",
            Source::Package => "the package's",
        }
    }
}

fn effective(set: &str, package: &Package) -> Effective {
    let stated = package
        .table
        .iter()
        .find(|(name, _)| name == set)
        .map(|(_, encodings)| encodings);
    let (framing, framing_from) = match stated.and_then(|s| s.framing.clone()) {
        Some(framing) => (framing, Source::Set),
        None => (package.wire.framing.clone(), Source::Package),
    };
    let (handshake, handshake_from) = match stated.and_then(|s| s.handshake.as_ref()) {
        Some((name, params)) => (
            HandshakeValue::with(name, params.iter().map(|(k, v)| (k.as_str(), v.as_str()))),
            Source::Set,
        ),
        None => (
            HandshakeValue::with(
                &package.wire.handshake,
                package
                    .wire
                    .handshake_params
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str())),
            ),
            Source::Package,
        ),
    };
    Effective {
        framing,
        framing_from,
        handshake,
        handshake_from,
    }
}

/// Why `name` can't be a set's framing, from its facts, or `None` if it can.
fn framing_problem(name: &str, encodings: &Encodings) -> Option<String> {
    let mut reasons = vec![];
    for (id, wanted, why) in FRAMING_NEEDS {
        match framing_fact(name, id, encodings) {
            Some(value) if value == *wanted => {}
            Some(value) => reasons.push(format!(
                "its `{id}` is `{value}`, and a set needs `{wanted}`: {why}"
            )),
            None => reasons.push(format!(
                "core can't read its `{id}`, which a set needs to be `{wanted}`: {why}"
            )),
        }
    }
    (!reasons.is_empty()).then(|| reasons.join("; "))
}

/// What the handshake `name` states as its `schema_comparison`: the profile's,
/// else the fact's default for a declared handshake that doesn't state it.
/// `None` for one with no profile.
fn schema_comparison(name: &str, encodings: &Encodings) -> Option<String> {
    if let Some(profile) = official::profile(EncodingKind::Handshake, name) {
        return profile.schema_comparison();
    }
    encodings.find(EncodingKind::Handshake, name).and_then(|_| {
        comline_catalog::fact(fact::SCHEMA_COMPARISON)?
            .default
            .map(String::from)
    })
}

/// Why `name` can't be a set's handshake, from its facts, or `None` if it can.
fn handshake_problem(name: &str, encodings: &Encodings) -> Option<String> {
    match schema_comparison(name, encodings).as_deref() {
        Some("per_protocol") => None,
        Some(other) => Some(format!(
            "its `schema_comparison` is `{other}`, so it judges the whole schema at once and a change in \
             one set would refuse every set; a set needs `per_protocol`"
        )),
        None => Some(
            "it has no profile and sends no Hello, so nothing would enforce the set; a set needs a handshake \
             whose `schema_comparison` is `per_protocol`"
                .to_string(),
        ),
    }
}

/// The official encodings of `kind` a set can have, by the same facts, for a
/// fix to name.
fn supported(kind: EncodingKind, encodings: &Encodings) -> Vec<&'static str> {
    official::names(kind)
        .into_iter()
        .filter(|name| match kind {
            EncodingKind::Framing => framing_problem(name, encodings).is_none(),
            EncodingKind::Handshake => handshake_problem(name, encodings).is_none(),
            EncodingKind::Format => false,
        })
        .collect()
}

fn fix_with_supported(encodings: &Encodings) -> String {
    let join = |names: Vec<&str>| {
        names
            .iter()
            .map(|n| format!("`{n}`"))
            .collect::<Vec<_>>()
            .join(" or ")
    };
    format!(
        "give the set a handshake that compares protocol by protocol ({}) and a framing whose protocol \
         numbers don't depend on position ({}) in `connections`, or in the package's `handshake` and \
         `wire`",
        join(supported(EncodingKind::Handshake, encodings)),
        join(supported(EncodingKind::Framing, encodings)),
    )
}

/// Why `name` can't name a set, or `None` if it can: it's a module's name in
/// each target, so it has to be one.
fn bad_name(name: &str) -> Option<String> {
    let snake_case = name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !name.ends_with('_')
        && !name.contains("__");
    if !snake_case {
        return Some(format!(
            "`{name}` isn't lowercase snake_case: lowercase letters, digits and single underscores, \
             starting with a letter"
        ));
    }
    for (target, words) in KEYWORDS {
        if words.contains(&name) {
            return Some(format!(
                "`{name}` is a keyword of {target}, and a set is a module of that name"
            ));
        }
    }
    if MODULE_ITEMS.contains(&name) {
        return Some(format!(
            "`{name}` is a name the connection module already has (`{}`), so a set of that name \
             would collide with it",
            MODULE_ITEMS.join("`, `")
        ));
    }
    None
}

/// Whether `protocol` runs on a connection: its framing, its own or the
/// package's, says whether a frame is a request or a reply, so it can share
/// a transport. Lockstep `datagram@1` doesn't (`directions = one`).
fn runs_on_a_connection(protocol: &Protocol, package: &Package) -> bool {
    let framing = own_framing(protocol).unwrap_or_else(|| package.wire.framing.clone());
    framing_fact(&framing, fact::DIRECTIONS, package.encodings).as_deref() != Some("one")
}

fn own_framing(protocol: &Protocol) -> Option<String> {
    FramingValue::of_protocol(&protocol.parameters).map(|framing| framing.name.to_string())
}

fn own_handshake(protocol: &Protocol) -> Option<HandshakeValue> {
    protocol.parameters.iter().find_map(|p| match p {
        FrozenUnit::Property { name, value } if name == "handshake" => HandshakeValue::of(value),
        _ => None,
    })
}

/// How a handshake reads in a message: its name, and the parameters that
/// aren't their defaults.
fn describe_handshake(handshake: &HandshakeValue) -> String {
    if handshake.params.is_empty() {
        format!("`{}`", handshake.name)
    } else {
        let params: Vec<String> = handshake
            .params
            .iter()
            .map(|(k, v)| format!("{k} = {v}"))
            .collect();
        format!("`{}` ({})", handshake.name, params.join(", "))
    }
}

/// Every error of the package's connection sets, none stopping the rest:
/// what its protocols write, what `connections` says, and the encodings of
/// each set. A package with no `@connection` has no sets, so nothing is
/// checked but a `connections` table, whose every entry names a set no
/// protocol is in.
pub fn check(protocols: &[Protocol], package: &Package) -> Vec<Finding> {
    let mut findings = vec![];
    let mut push = |schema: Option<usize>, diagnostic: Diagnostic| {
        findings.push(Finding { schema, diagnostic })
    };
    let first_annotated = protocols.iter().find(|p| p.connection.is_some());

    // Each name once, though several protocols write it: what is wrong with a
    // set is wrong with it, not with its members.
    let mut used: BTreeSet<String> = BTreeSet::new();
    let mut named_badly: BTreeSet<String> = BTreeSet::new();
    let mut set_checked: BTreeSet<String> = BTreeSet::new();

    for protocol in protocols {
        let context = format!("protocol '{}'", protocol.name);
        let Some(reading) = &protocol.connection else {
            // All or nothing: with any protocol in a set, one that runs on a
            // connection and is in none is an error.
            if let Some(annotated) = first_annotated {
                if runs_on_a_connection(protocol, package) {
                    push(
                        Some(protocol.schema),
                        Diagnostic::new(
                            UNASSIGNED,
                            format!(
                                "protocol `{}` runs on a connection but is in no set, and `{}` is in one: \
                                 once a protocol has `@connection`, every protocol that runs on a \
                                 connection needs one",
                                protocol.name, annotated.name
                            ),
                        )
                        .with_context(context)
                        .with_help(format!(
                            "add `@{CONNECTION} = <set>` to `{}`, or remove `@{CONNECTION}` from the others",
                            protocol.name
                        ))
                        .with_span(protocol.span),
                    );
                }
            }
            continue;
        };
        let names = match reading {
            Reading::Sets(names) => names,
            Reading::Empty(diagnostic) | Reading::Invalid(diagnostic) => {
                push(Some(protocol.schema), diagnostic.clone());
                continue;
            }
        };

        // A protocol on no connection can't be in a set, and that is all that
        // is said of it.
        if !runs_on_a_connection(protocol, package) {
            used.extend(names.iter().map(|n| n.text.clone()));
            let framing = own_framing(protocol).unwrap_or_else(|| package.wire.framing.clone());
            let title = official::title(EncodingKind::Framing, &framing)
                .unwrap_or(&framing)
                .to_string();
            push(
                Some(protocol.schema),
                Diagnostic::new(
                    NOT_ON_A_CONNECTION,
                    format!(
                        "protocol `{}` runs on no connection (its framing, {title}, carries one protocol \
                         per transport), so it can't be in a set",
                        protocol.name
                    ),
                )
                .with_context(context)
                .with_help(format!("remove `@{CONNECTION}` from `{}`", protocol.name))
                .with_span(protocol.span),
            );
            continue;
        }

        // The sets it names, each once.
        let mut sets: Vec<&SetName> = vec![];
        for name in names {
            if sets.iter().any(|s| s.text == name.text) {
                push(
                    Some(protocol.schema),
                    Diagnostic::new(
                        DUPLICATE_SET,
                        format!(
                            "`{}` is named twice in the `@{CONNECTION}` of protocol `{}`",
                            name.text, protocol.name
                        ),
                    )
                    .with_context(context.clone())
                    .with_help(format!("name `{}` once", name.text))
                    .with_span(name.span),
                );
                continue;
            }
            sets.push(name);
        }

        for set in &sets {
            used.insert(set.text.clone());
            if let Some(why) = bad_name(&set.text) {
                if named_badly.insert(set.text.clone()) {
                    push(
                        Some(protocol.schema),
                        Diagnostic::new(BAD_SET_NAME, format!("`{}` can't name a set: {why}", set.text))
                            .with_context(context.clone())
                            .with_help("rename the set to a lowercase snake_case name that isn't a keyword or a name of the connection module")
                            .with_span(set.span),
                    );
                }
            }
            // The encodings are the set's, so each set is judged once.
            if set_checked.insert(set.text.clone()) {
                let set_encodings = effective(&set.text, package);
                if let Some(why) = framing_problem(&set_encodings.framing, package.encodings) {
                    push(
                        Some(protocol.schema),
                        Diagnostic::new(
                            UNSUPPORTED_ENCODING,
                            format!(
                                "set `{}` can't have the framing `{}` ({}): {why}",
                                set.text,
                                set_encodings.framing,
                                set_encodings.framing_from.whose()
                            ),
                        )
                        .with_context(context.clone())
                        .with_help(fix_with_supported(package.encodings))
                        .with_span(set.span),
                    );
                }
                if let Some(why) =
                    handshake_problem(&set_encodings.handshake.name, package.encodings)
                {
                    push(
                        Some(protocol.schema),
                        Diagnostic::new(
                            UNSUPPORTED_ENCODING,
                            format!(
                                "set `{}` can't have the handshake `{}` ({}): {why}",
                                set.text,
                                set_encodings.handshake.name,
                                set_encodings.handshake_from.whose()
                            ),
                        )
                        .with_context(context.clone())
                        .with_help(fix_with_supported(package.encodings))
                        .with_span(set.span),
                    );
                }
            }
        }

        // What the protocol pins of its own has to be a set's too: a
        // framing or handshake no set can have is refused as that, and one a
        // set can have has to be the set's.
        let effectives: Vec<(&SetName, Effective)> = sets
            .iter()
            .map(|s| (*s, effective(&s.text, package)))
            .collect();

        if let Some(framing) = own_framing(protocol) {
            if let Some(why) = framing_problem(&framing, package.encodings) {
                push(
                    Some(protocol.schema),
                    Diagnostic::new(
                        UNSUPPORTED_ENCODING,
                        format!(
                            "protocol `{}` writes the framing `{framing}` in `@framing`, which a set can't have: {why}",
                            protocol.name
                        ),
                    )
                    .with_context(context.clone())
                    .with_help(format!(
                        "remove `@framing` so the protocol takes its set's, or give it a framing a set can have. {}",
                        fix_with_supported(package.encodings)
                    ))
                    .with_span(protocol.span),
                );
            } else {
                // Sets whose framing is one a set can have, which it is held to.
                let comparable: Vec<&(&SetName, Effective)> = effectives
                    .iter()
                    .filter(|(_, e)| framing_problem(&e.framing, package.encodings).is_none())
                    .collect();
                let differing: Vec<&str> = comparable
                    .iter()
                    .filter(|(_, e)| e.framing != framing)
                    .map(|(s, _)| s.text.as_str())
                    .collect();
                if !differing.is_empty() {
                    let own_sets_differ = comparable
                        .windows(2)
                        .any(|pair| pair[0].1.framing != pair[1].1.framing);
                    push(
                        Some(protocol.schema),
                        mismatch(
                            protocol,
                            if own_sets_differ {
                                format!(
                                    "protocol `{}` is in sets whose framings differ, so it can't write its own \
                                     `@framing`",
                                    protocol.name
                                )
                            } else {
                                format!(
                                    "protocol `{}` writes the framing `{framing}`, which isn't its set's: {}",
                                    protocol.name,
                                    comparable
                                        .iter()
                                        .filter(|(s, _)| differing.contains(&s.text.as_str()))
                                        .map(|(s, e)| format!("set `{}` has `{}`", s.text, e.framing))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                )
                            },
                            "framing",
                        ),
                    );
                }
            }
        }

        if let Some(handshake) = own_handshake(protocol) {
            if let Some(why) = handshake_problem(&handshake.name, package.encodings) {
                push(
                    Some(protocol.schema),
                    Diagnostic::new(
                        UNSUPPORTED_ENCODING,
                        format!(
                            "protocol `{}` writes the handshake {} in `@handshake`, which a set can't have: {why}",
                            protocol.name,
                            describe_handshake(&handshake)
                        ),
                    )
                    .with_context(context.clone())
                    .with_help(format!(
                        "remove `@handshake` so the protocol takes its set's, or give it a handshake a set can have. {}",
                        fix_with_supported(package.encodings)
                    ))
                    .with_span(protocol.span),
                );
            } else {
                // A handshake agrees when its name and wire parameters do;
                // policies are each end's own, and may differ.
                let comparable: Vec<&(&SetName, Effective)> = effectives
                    .iter()
                    .filter(|(_, e)| {
                        handshake_problem(&e.handshake.name, package.encodings).is_none()
                    })
                    .collect();
                let differing: Vec<&&(&SetName, Effective)> = comparable
                    .iter()
                    .filter(|(_, e)| !e.handshake.same_wire(&handshake))
                    .collect();
                if !differing.is_empty() {
                    let own_sets_differ = comparable
                        .windows(2)
                        .any(|pair| !pair[0].1.handshake.same_wire(&pair[1].1.handshake));
                    push(
                        Some(protocol.schema),
                        mismatch(
                            protocol,
                            if own_sets_differ {
                                format!(
                                    "protocol `{}` is in sets whose handshakes differ, so it can't write its own \
                                     `@handshake`",
                                    protocol.name
                                )
                            } else {
                                format!(
                                    "protocol `{}` writes the handshake {}, which isn't its set's: {}",
                                    protocol.name,
                                    describe_handshake(&handshake),
                                    differing
                                        .iter()
                                        .map(|(s, e)| format!(
                                            "set `{}` has {}",
                                            s.text,
                                            describe_handshake(&e.handshake)
                                        ))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                )
                            },
                            "handshake",
                        ),
                    );
                }
            }
        }
    }

    // The table names the sets that exist, which are the ones a protocol is in.
    for (set, _) in package.table {
        if !used.contains(set) {
            let reason = if first_annotated.is_none() {
                "no protocol has `@connection`"
            } else {
                "no protocol is in a set of that name"
            };
            push(
                None,
                Diagnostic::new(
                    UNKNOWN_SET,
                    format!("`connections` has an entry for `{set}`, but {reason}"),
                )
                .with_context("config.idp's `connections`")
                .with_help(format!(
                    "remove the entry, or put a protocol in the set with `@{CONNECTION} = {set}`"
                )),
            );
        }
    }

    findings
}

fn mismatch(protocol: &Protocol, message: String, part: &str) -> Diagnostic {
    Diagnostic::new(ENCODING_MISMATCH, message)
        .with_context(format!("protocol '{}'", protocol.name))
        .with_help(format!(
            "remove the protocol's own `@{part}`, which its set's applies to it, or change the set's `{part}` in \
             `connections`"
        ))
        .with_span(protocol.span)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_is_named_in_lowercase_snake_case() {
        for good in ["admin", "admin_api2", "a", "v2", "public_health"] {
            assert!(bad_name(good).is_none(), "{good}");
        }
        for bad in [
            "Admin",
            "adminApi",
            "_admin",
            "2admin",
            "admin_",
            "admin__api",
            "admin-api",
            "",
        ] {
            let why = bad_name(bad).unwrap_or_default();
            assert!(why.contains("snake_case"), "{bad}: {why}");
        }
    }

    #[test]
    fn a_keyword_of_any_target_is_refused_and_the_message_says_whose() {
        assert!(bad_name("loop").unwrap().contains("Rust"));
        assert!(bad_name("interface").unwrap().contains("TypeScript"));
        assert!(bad_name("default").unwrap().contains("TypeScript"));
        assert!(bad_name("serve").unwrap().contains("connection module"));
        assert!(bad_name("services").unwrap().contains("connection module"));
        assert!(bad_name("client_services")
            .unwrap()
            .contains("connection module"));
    }

    /// The rule is the facts': it holds today's encodings to what the
    /// specification's profiles state, and no name decides it.
    #[test]
    fn what_a_set_needs_is_read_from_the_facts_of_the_official_encodings() {
        let encodings = Encodings::default();
        assert_eq!(
            supported(EncodingKind::Handshake, &encodings),
            ["comline@2"]
        );
        assert_eq!(supported(EncodingKind::Framing, &encodings), ["datagram@3"]);
        assert!(handshake_problem("comline@1", &encodings)
            .unwrap()
            .contains("whole_schema"));
        assert!(handshake_problem("none", &encodings)
            .unwrap()
            .contains("no profile"));
        assert!(framing_problem("datagram@2", &encodings)
            .unwrap()
            .contains("`calls` is `by_index`"));
        assert!(framing_problem("datagram", &encodings)
            .unwrap()
            .contains("`directions` is `one`"));
        assert!(framing_problem("jsonrpc", &encodings)
            .unwrap()
            .contains("`protocol` is `unnamed`"));
    }
}
