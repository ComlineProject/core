//! Strict mode for `@unverified` (core#222, item 2): `annotations.unverified.strict
//! = true`.
//!
//! By default an outer marker covers what is inside it, and tooling suggests
//! the narrower placement. Strict asks for two things at every marker it
//! applies to:
//!
//! - **the keyed form** (`unverified::strict-keyed`): a plain
//!   `@unverified = "reason"` is an error, and says the keyed rewrite for what
//!   it covers where it sits;
//! - **the innermost scope** (`unverified::strict-scope`): each guarantee
//!   acknowledged at the innermost scope the catalog allows on the path to its
//!   cause. `bounded_size` is judged by the coverage pass, which knows the
//!   paths ([`super::bounds`]); what a package's own settings give up is judged
//!   here ([`check_package_default`]): it is acknowledged next to the setting,
//!   in `config.idp`, not on each protocol that takes it.
//!
//! The setting is resolved where `annotations.unverified.allowed` is: the
//! package's `settings`, a schema's own block, a `@settings = Name` preset on
//! the declaration, and a key scoped to the declaration site
//! (`struct.field.annotations.unverified.strict`), the scoped key first. It is
//! the *marker's* site that decides whether the marker is held to strict.

use comline_catalog::Scope;

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::schema::ir::frozen::unit::{ConfigValue, FrozenUnit, Guarantee};
use crate::schema::ir::unverified::{self, Encodings, Inherits};
use crate::settings::effective::{
    effective_declaration_settings, effective_schema_settings, schema_settings,
    settings_annotation_of, SchemaSettings,
};
use crate::settings::keys::{self, DeclPath};
use crate::settings::value::{SettingsDict, SettingsValue};

pub(crate) const STRICT_KEYED: DiagnosticCode = DiagnosticCode::error("unverified::strict-keyed");
pub(crate) const STRICT_SCOPE: DiagnosticCode = DiagnosticCode::error("unverified::strict-scope");

/// Where `annotations.unverified.strict` is in effect, for one schema.
#[derive(Debug, Clone, Default)]
pub struct Strict {
    schema: SchemaSettings,
    effective: SettingsDict,
}

impl Strict {
    /// What the package's settings and `units`' own `settings` blocks say.
    pub fn of(units: &[FrozenUnit], package_settings: &SettingsDict) -> Self {
        let schema = schema_settings(units);
        let effective = effective_schema_settings(package_settings, &schema);
        Strict { schema, effective }
    }

    /// Whether a marker on a declaration with these `parameters` (its
    /// `@settings` preset is read off them) at `decl` is held to strict.
    pub fn at(&self, parameters: &[FrozenUnit], decl: DeclPath) -> bool {
        let settings = effective_declaration_settings(
            &self.effective,
            &self.schema,
            settings_annotation_of(parameters),
        );
        resolve(&settings, decl)
    }
}

/// Whether the package's own settings are strict, where there is no
/// declaration: its `unverified` in `config.idp`.
pub fn package_strict(package_settings: &SettingsDict) -> bool {
    matches!(
        package_settings.get_path(&keys::annotations_strict().key(None, Some(unverified::KEY))),
        Some(SettingsValue::Bool(true))
    )
}

/// The most specific `strict` in `settings` for `decl`; a value that isn't a
/// boolean is the settings pass's error, and counts as unset here.
fn resolve(settings: &SettingsDict, decl: DeclPath) -> bool {
    keys::annotations_strict()
        .candidates(decl, unverified::KEY)
        .iter()
        .find_map(|key| match settings.get_path(key) {
            Some(SettingsValue::Bool(strict)) => Some(*strict),
            _ => None,
        })
        .unwrap_or(false)
}

/// `"reason"` as it is written, quotes and backslashes escaped.
fn quoted(reason: &str) -> String {
    format!("\"{}\"", reason.replace('\\', "\\\\").replace('"', "\\\""))
}

/// `@unverified(a = "reason", b = "reason")`.
fn keyed(guarantees: &[Guarantee], reason: &str) -> String {
    let entries: Vec<String> = guarantees
        .iter()
        .map(|g| format!("{} = {}", g.name(), quoted(reason)))
        .collect();
    format!("@unverified({})", entries.join(", "))
}

fn plain_marker(
    label: &str,
    reason: &str,
    covers: &[Guarantee],
    span: Option<(usize, usize)>,
) -> Diagnostic {
    let help = if covers.is_empty() {
        "it gives up nothing here: remove it".to_string()
    } else {
        format!("name what it gives up: `{}`", keyed(covers, reason))
    };
    Diagnostic::new(
        STRICT_KEYED,
        format!(
            "`@unverified` on {label} is a plain reason, and `annotations.unverified.strict` requires the keyed form"
        ),
    )
    .with_help(help)
    .maybe_span(span)
}

/// What a plain marker at `scope` covers, as far as a guarantee gives that
/// scope: `bounded_size` is the only one that has a cause below the protocol.
fn inner_covers(scope: Scope) -> Vec<Guarantee> {
    if Guarantee::BoundedSize.entry().allows_scope(scope) {
        vec![Guarantee::BoundedSize]
    } else {
        vec![]
    }
}

/// A plain `@unverified` where strict applies, anywhere in `units`: the keyed
/// rewrite for what it covers there. Run on the schema once its protocols'
/// given-up sets and acknowledgements are final.
pub fn check_keyed(units: &[FrozenUnit], strict: &Strict) -> Vec<Diagnostic> {
    let mut check = Keyed {
        strict,
        errors: vec![],
    };
    for unit in units {
        match unit {
            FrozenUnit::Protocol {
                name,
                parameters,
                functions,
                span,
                ..
            } => {
                // What the plain marker covers: what is acknowledged at the
                // protocol, which is what the marker said of each.
                let at_protocol = unverified::acknowledgements(parameters);
                let covers = unverified::frozen_set(parameters)
                    .iter()
                    .copied()
                    .filter(|g| {
                        at_protocol
                            .iter()
                            .any(|a| a.guarantee == *g && a.scope == Scope::Protocol)
                    })
                    .collect();
                check.site(
                    parameters,
                    DeclPath::Protocol,
                    format!("protocol `{name}`"),
                    covers,
                    Some(*span),
                );
                for function in functions {
                    let FrozenUnit::Function {
                        name: f,
                        parameters,
                        arguments,
                        span,
                        ..
                    } = function
                    else {
                        continue;
                    };
                    let path = format!("{name}.{f}");
                    let covers = inner_covers(Scope::Function);
                    check.site(
                        parameters,
                        DeclPath::ProtocolFunction,
                        format!("function `{path}`"),
                        covers,
                        Some(*span),
                    );
                    for argument in arguments {
                        let label = format!("argument `{}` of `{path}`", argument.name);
                        let covers = inner_covers(Scope::Argument);
                        check.site(
                            &argument.parameters,
                            DeclPath::ProtocolFunction,
                            label,
                            covers,
                            Some(argument.span),
                        );
                    }
                }
            }
            FrozenUnit::Struct {
                name,
                parameters,
                fields,
                span,
                ..
            } => {
                check.site(
                    parameters,
                    DeclPath::Struct,
                    format!("struct `{name}`"),
                    inner_covers(Scope::Type),
                    Some(*span),
                );
                check.fields(fields, DeclPath::StructField, name);
            }
            FrozenUnit::Error {
                name,
                parameters,
                fields,
                span,
                imported_from: None,
                ..
            } => {
                check.site(
                    parameters,
                    DeclPath::Error,
                    format!("error `{name}`"),
                    inner_covers(Scope::Type),
                    *span,
                );
                check.fields(fields, DeclPath::ErrorField, name);
            }
            FrozenUnit::TypeAlias {
                name,
                parameters,
                span,
                ..
            } => {
                check.site(
                    parameters,
                    DeclPath::Struct,
                    format!("type `{name}`"),
                    inner_covers(Scope::Type),
                    Some(*span),
                );
            }
            _ => {}
        }
    }
    check.errors
}

struct Keyed<'a> {
    strict: &'a Strict,
    errors: Vec<Diagnostic>,
}

impl Keyed<'_> {
    fn site(
        &mut self,
        parameters: &[FrozenUnit],
        decl: DeclPath,
        label: String,
        covers: Vec<Guarantee>,
        span: Option<(usize, usize)>,
    ) {
        if let Some(ConfigValue::Str(reason)) = unverified::marker(parameters) {
            if self.strict.at(parameters, decl) {
                self.errors
                    .push(plain_marker(&label, reason, &covers, span));
            }
        }
    }

    fn fields(&mut self, fields: &[FrozenUnit], decl: DeclPath, owner: &str) {
        for field in fields {
            if let FrozenUnit::Field {
                name,
                parameters,
                span,
                ..
            } = field
            {
                let label = format!("field `{name}` of `{owner}`");
                self.site(
                    parameters,
                    decl,
                    label,
                    inner_covers(Scope::Field),
                    Some(*span),
                );
            }
        }
    }
}

/// The package's `unverified` as a plain reason, under a strict package: the
/// keyed rewrite for what it covers, which is what the package's settings give
/// up for the protocols that take them (`covered`), else every guarantee the
/// catalog gives package scope that a protocol can inherit.
pub fn check_package_keyed(package: &ConfigValue, covered: &[Guarantee]) -> Option<Diagnostic> {
    let ConfigValue::Str(reason) = package else {
        return None;
    };
    let help = if covered.is_empty() {
        "it covers nothing: no protocol takes a package setting that gives anything up".to_string()
    } else {
        let entries: Vec<String> = covered
            .iter()
            .map(|g| format!("{} = {}", g.name(), quoted(reason)))
            .collect();
        format!(
            "name what it covers: `unverified = {{ {} }}`",
            entries.join(", ")
        )
    };
    // Rendered without a source, where only the message is shown: it carries
    // the rewrite too.
    Some(
        Diagnostic::new(
            STRICT_KEYED,
            format!(
                "config.idp's `unverified` is a plain reason, and `annotations.unverified.strict` requires the keyed form: {help}"
            ),
        )
        .with_help(help),
    )
}

/// A protocol's own marker acknowledging what the package's settings give up,
/// where strict applies: the cause is the package's setting (`handshake =
/// "none"`, a `wire.format` without vectors), so its innermost scope is the
/// package, and the acknowledgement belongs in `config.idp` beside it.
///
/// `units` are a schema's protocols as the freeze saw them, `inherits` what
/// each takes from the package ([`unverified::inherits`]).
pub fn check_package_default(
    units: &[FrozenUnit],
    strict: &Strict,
    encodings: &Encodings,
    inherits: &[Inherits],
) -> Vec<Diagnostic> {
    let mut errors = vec![];
    let mut inherits = inherits.iter();
    for unit in units {
        let FrozenUnit::Protocol {
            name,
            parameters,
            span,
            ..
        } = unit
        else {
            continue;
        };
        let inherited = inherits.next().copied().unwrap_or_default();
        if !strict.at(parameters, DeclPath::Protocol) {
            continue;
        }
        let Some(marker) = unverified::marker(parameters) else {
            continue;
        };
        let acknowledged: Vec<(Guarantee, String)> =
            unverified::package_caused(parameters, encodings, inherited)
                .into_iter()
                .filter(|(g, _)| unverified::acknowledges(marker, *g))
                .collect();
        if acknowledged.is_empty() {
            continue;
        }
        let names: Vec<String> = acknowledged
            .iter()
            .map(|(g, _)| format!("`{}`", g.name()))
            .collect();
        let causes: Vec<String> =
            acknowledged
                .iter()
                .map(|(_, why)| why.clone())
                .fold(vec![], |mut all, why| {
                    if !all.contains(&why) {
                        all.push(why);
                    }
                    all
                });
        let entries: Vec<String> = acknowledged
            .iter()
            .map(|(g, _)| format!("{} = \"…\"", g.name()))
            .collect();
        errors.push(
            Diagnostic::new(
                STRICT_SCOPE,
                format!(
                    "`@unverified` on protocol `{name}` acknowledges {}, which come{} from the package's own settings ({}), and `annotations.unverified.strict` requires the innermost scope: the package",
                    names.join(", "),
                    if acknowledged.len() == 1 { "s" } else { "" },
                    causes.join("; "),
                ),
            )
            .with_help(format!("move it next to the setting, in config.idp: `unverified = {{ {} }}`", entries.join(", ")))
            .with_span(*span),
        );
    }
    errors
}
