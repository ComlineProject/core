// Standard Uses
use std::collections::{HashMap, HashSet};

// Crate Uses
use crate::diagnostics::{Diagnostic, DiagnosticCode};
use super::symbols::{SymbolTable, SymbolType};
use crate::schema::ir::frozen::unit::{schema_namespace, ConfigValue, DeprecatedValue, FrozenUnit};
use crate::schema::ir::compiler::interpreter::incremental::{NOT_AN_ERROR_PREFIX, UNRESOLVED_PREFIX};
use crate::schema::ir::frozen::types::{Literal, PrimitiveType, TypeRef};
use crate::utils::text::{closest, levenshtein_distance};
use crate::config_value::{self, ValueType};
use crate::schema::idl::annotations;

const DUPLICATE_DECLARATION: DiagnosticCode = DiagnosticCode::error("validation::duplicate-declaration");
const INVALID_CONSTANT_TYPE: DiagnosticCode = DiagnosticCode::error("validation::invalid-constant-type");
const VALUE_OUT_OF_RANGE: DiagnosticCode =
    DiagnosticCode::error("validation::value-out-of-range");
const LITERAL_TYPE_MISMATCH: DiagnosticCode =
    DiagnosticCode::error("validation::literal-type-mismatch");
const UNKNOWN_VALIDATOR_PROPERTY: DiagnosticCode =
    DiagnosticCode::error("validation::unknown-validator-property");
const INVALID_VALIDATOR_REFERENCE: DiagnosticCode =
    DiagnosticCode::error("validation::invalid-validator-reference");
const DUPLICATE_VALIDATOR_ARGUMENT: DiagnosticCode =
    DiagnosticCode::error("validation::duplicate-validator-argument");
const UNKNOWN_VALIDATOR_ARGUMENT: DiagnosticCode =
    DiagnosticCode::error("validation::unknown-validator-argument");
const VALIDATOR_ARGUMENT_TYPE_MISMATCH: DiagnosticCode =
    DiagnosticCode::error("validation::validator-argument-type-mismatch");
const NOT_A_VALIDATOR: DiagnosticCode = DiagnosticCode::error("validation::not-a-validator");
const UNKNOWN_VALIDATOR: DiagnosticCode = DiagnosticCode::error("validation::unknown-validator");
const STRUCT_CYCLE: DiagnosticCode = DiagnosticCode::error("validation::struct-cycle");
const UNKNOWN_TYPE: DiagnosticCode = DiagnosticCode::error("validation::unknown-type");
const NOT_A_TYPE: DiagnosticCode = DiagnosticCode::error("validation::not-a-type");
const DEPRECATED_TYPE_USAGE: DiagnosticCode =
    DiagnosticCode::warning("validation::deprecated-type-usage");
const DEPRECATED_MALFORMED_VALUE: DiagnosticCode =
    DiagnosticCode::error("validation::deprecated-malformed-value");
const ANNOTATION_VALUE_MISMATCH: DiagnosticCode =
    DiagnosticCode::error("validation::annotation-value-mismatch");
const UNRESOLVED_THROW: DiagnosticCode = DiagnosticCode::error("validation::unresolved-throw");
const THROW_NOT_AN_ERROR: DiagnosticCode = DiagnosticCode::error("validation::throw-not-an-error");
const DUPLICATE_THROW: DiagnosticCode = DiagnosticCode::error("validation::duplicate-throw");
const VALIDATOR_MISSING_TARGET: DiagnosticCode =
    DiagnosticCode::error("validation::validator-missing-target");
const VALIDATOR_TARGET_MISMATCH: DiagnosticCode =
    DiagnosticCode::error("validation::validator-target-mismatch");
const VALIDATOR_ACCESSOR_UNSUPPORTED: DiagnosticCode =
    DiagnosticCode::error("validation::validator-accessor-unsupported");

/// What validation needs to know about names a schema imports but whose
/// declarations aren't among its own units. Resolution is left to the
/// caller, the same split as `size.rs`'s `SizeLookup`: a build answers from
/// its `ProjectContext`, the language server from its project, and plain
/// [`validate`] answers nothing - an imported name stays unchecked, as it
/// always was.
///
/// Types come back already resolved through aliases in the schema that
/// declares them.
pub trait ImportLookup {
    /// The `for` targets of an imported validator, as written or qualified
    /// (`StringBounds`, `validators::StringBounds`).
    fn validator_targets(&self, name: &str) -> Option<Vec<TypeRef>>;
    /// What an imported `type` alias stands for.
    fn alias_target(&self, name: &str) -> Option<TypeRef>;
}

/// The lookup for a schema validated on its own: no imports known.
pub struct NoImports;

impl ImportLookup for NoImports {
    fn validator_targets(&self, _: &str) -> Option<Vec<TypeRef>> {
        None
    }
    fn alias_target(&self, _: &str) -> Option<TypeRef> {
        None
    }
}



pub fn validate(units: &[FrozenUnit]) -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
    validate_with(units, &NoImports)
}

/// [`validate`], with `imports` answering for names this schema imports.
pub fn validate_with(
    units: &[FrozenUnit],
    imports: &dyn ImportLookup,
) -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
    let mut errors = vec![];
    let mut warnings = vec![];
    let mut symbols = SymbolTable::new();
    let mut deprecated: HashMap<&str, Option<String>> = HashMap::new();
    // The schema this is, for naming a declaration the way the author would
    // have to write it from elsewhere. `None` on the paths that freeze
    // declarations without a project context (most of this crate's own
    // tests, and the editor before it knows the file's module path), where
    // the bare name is all there is to say.
    let namespace = schema_namespace(units).filter(|ns| !ns.is_empty());
    let qualified = |name: &str| match namespace {
        Some(ns) => format!("{ns}::{name}"),
        None => name.to_string(),
    };

    // Pass 1: Collect Symbols & Check Duplicates
    for unit in units {
        // Imports: the resolved path is a symbol (so a qualified `ns::Name`
        // resolves), and the bare name a `use` brought into scope goes in a
        // separate set (so it never conflicts and a local declaration shadows
        // it). Redundant identical `use`s are fine, hence `let _`.
        if let FrozenUnit::Import(path, alias, _span) = unit {
            let _ = symbols.insert(path.as_str(), SymbolType::Import, Some(*_span));
            let bare = match alias {
                Some(a) => Some(a.as_str()),
                None => path.rsplit("::").next().filter(|s| *s != path.as_str()),
            };
            if let Some(bare) = bare {
                symbols.add_bare_import(bare);
            }
            continue;
        }

        let (name, kind, span) = match unit {
            FrozenUnit::Struct { name, span, .. } => (name.as_str(), SymbolType::Struct, Some(*span)),
            FrozenUnit::Enum { name, span, .. } => (name.as_str(), SymbolType::Enum, Some(*span)),
            FrozenUnit::Protocol { name, span, .. } => (name.as_str(), SymbolType::Protocol, Some(*span)),
            FrozenUnit::Constant { name, span, .. } => (name.as_str(), SymbolType::Constant, Some(*span)),
            FrozenUnit::Validator { name, span, .. } => {
                (name.as_str(), SymbolType::Validator, Some(*span))
            }
            FrozenUnit::TypeAlias { name, span, .. } => (name.as_str(), SymbolType::TypeAlias, Some(*span)),
            // An error shares this one namespace with every other
            // declaration, because it generates a type named after itself -
            // `export interface Rejected` in TypeScript, `pub struct
            // Rejected` in Rust. A `struct Test` next to an `error Test`
            // therefore emitted two definitions of `Test` and the generated
            // code did not compile; nothing here rejected it, and two
            // `error Test` were not caught either.
            // A re-export slot (`imported_from: Some(_)`) is not a local
            // declaration - it is the ordinal this schema reserved for
            // another schema's error, created by `throws` resolution rather
            // than written here - so it must not collide with a local
            // declaration of that name. Its `span` is `None` for the same
            // reason: it was written in another file.
            FrozenUnit::Error { name, imported_from: None, span, .. } => {
                (name.as_str(), SymbolType::Error, *span)
            }
            // TODO: Function handling if they become top-level
            _ => continue,
        };

        // Both sites, named by keyword and qualified by namespace: a
        // duplicate is a two-site problem, and "Definition of Error 'Test'"
        // (a `{:?}` dump, unqualified) left the Problems panel with no way
        // to tell which `Test` in which schema it meant. The primary span
        // is the declaration that collided; the secondary is the one
        // already holding the name.
        if let Err(existing) = symbols.insert(name, kind, span) {
            let qualified_name = qualified(name);
            errors.push(
                Diagnostic::new(
                    DUPLICATE_DECLARATION,
                    format!("Duplicate definition of '{qualified_name}'"),
                )
                .with_context(format!("{} '{qualified_name}'", kind.keyword()))
                .with_help(format!(
                    "rename this {} or remove it - the {} above already defines '{name}'",
                    kind.keyword(),
                    existing.kind.keyword(),
                ))
                .maybe_span(span)
                .maybe_secondary(
                    existing.span,
                    format!("{} '{qualified_name}' declared here", existing.kind.keyword()),
                ),
            );
        }

        // Deprecation tracking stays struct-only. Which kinds are usable in
        // a type position is `SymbolType::usable_as_type`'s business now,
        // but a *usage site* is still only reachable for a struct: a
        // function's `! Name` throws is already an ordinal by the time
        // `validate` runs (`interpreter::incremental` resolves it), so
        // `@deprecated` on an error, a protocol, a field, or a function
        // parses fine and has nothing for this check to flag.
        if let FrozenUnit::Struct { parameters, .. } = unit {
            match deprecation_of(parameters, span) {
                Ok(Some(message)) => {
                    deprecated.insert(name, message);
                }
                Ok(None) => {}
                Err(diagnostic) => errors.push(diagnostic),
            }
        }
    }

    // Stop if duplicate errors found (avoids cascading errors)
    if !errors.is_empty() {
        return Err(errors);
    }

    // Pass 2: Type Resolution & Usage
    for unit in units {
        match unit {
            FrozenUnit::Struct { name, fields, .. } => {
                for field in fields {
                    match field {
                        FrozenUnit::Field { name: field_name, ty, default, span, .. } => {
                            let context = format!("Struct '{}', field '{}'", name, field_name);
                            validate_type(ty, &symbols, &deprecated, &mut errors, &mut warnings, &context, *span);
                            if let Some(default) = default {
                                check_literal(ty, default, &context, *span, &mut errors);
                            }
                        }
                        _ => {}
                    }
                }
            }
            FrozenUnit::Protocol { name, functions, .. } => {
                for func in functions {
                    match func {
                        FrozenUnit::Function { name: func_name, arguments, _return, span, .. } => {
                            for arg in arguments {
                                validate_type(&arg.ty, &symbols, &deprecated, &mut errors, &mut warnings, &format!("Protocol '{}', function '{}', arg '{}'", name, func_name, arg.name), arg.span);
                            }
                            if let Some(ret_type) = _return {
                                validate_type(ret_type, &symbols, &deprecated, &mut errors, &mut warnings, &format!("Protocol '{}', function '{}' return", name, func_name), *span);
                            }
                        }
                        _ => {}
                    }
                }
            }
            FrozenUnit::Constant { name, ty, value, span, .. } => {
                let context = format!("Constant '{name}'");
                match (ty.as_primitive(), value) {
                    // A constant is a primitive with a literal value - the
                    // literal's own fit is `check_literal`'s business.
                    (Some(_), Some(literal)) if !matches!(literal, Literal::Reference(_)) => {
                        check_literal(ty, literal, &context, *span, &mut errors);
                    }
                    (Some(_), _) => errors.push(
                        Diagnostic::new(
                            INVALID_CONSTANT_TYPE,
                            format!("Constant '{name}' must have a literal value"),
                        )
                        .with_context(context)
                        .with_span(*span),
                    ),
                    (None, _) => errors.push(Diagnostic { code: INVALID_CONSTANT_TYPE, help: text_type_hint(&ty.to_string()), secondary: None, severity_override: None,
                        message: format!("Constant '{}' cannot be a named type '{}' - only primitives allowed", name, ty),
                        context,
                        span: Some(*span),
                    }),
                }
            }
            FrozenUnit::TypeAlias { name, target, span, .. } => {
                // Catches an unresolvable target even on a path that skips
                // `alias_resolution::check_aliases`'s own pre-freeze check
                // (e.g. `Compile::from_source`/`from_declarations`, used
                // directly by most of this crate's own tests) - without
                // this, a bad target would otherwise only be caught at
                // each individual *use* site, if even that (a field typed
                // as this very alias validates fine on its own, since the
                // alias's own name is now a registered symbol).
                validate_type(target, &symbols, &deprecated, &mut errors, &mut warnings, &format!("Type alias '{}'", name), *span);
            }
            _ => {}
        }
    }

    // Pass 2.5: every `@validators = [Name(...)]` on a struct/error field must
    // name a declared `validator`, and (for locally declared ones) its keyword
    // arguments must be that validator's properties.
    let validator_fields: HashMap<&str, &[FrozenUnit]> = units
        .iter()
        .filter_map(|u| match u {
            FrozenUnit::Validator { name, properties, .. } => {
                Some((name.as_str(), properties.as_slice()))
            }
            _ => None,
        })
        .collect();
    let validator_props: HashMap<&str, Vec<&str>> = validator_fields
        .iter()
        .map(|(&v, props)| (v, prop_names(props)))
        .collect();

    // Pass 2.6: a `validate {}` block references only `value.*` and this
    // validator's own `params.*`.
    for unit in units {
        let FrozenUnit::Validator { name: vname, expression_block, .. } = unit else {
            continue;
        };
        let props = validator_props
            .get(vname.as_str())
            .cloned()
            .unwrap_or_default();
        let FrozenUnit::ExpressionBlock { asserts } = &**expression_block else {
            continue;
        };
        let vctx = format!("Validator '{}'", vname);
        for a in asserts {
            let FrozenUnit::Assert { references, .. } = a else {
                continue;
            };
            for path in references {
                let mut segs = path.split('.');
                match segs.next().unwrap_or("") {
                    "value" => {}
                    "params" => {
                        if let Some(prop) = segs.next() {
                            if !props.contains(&prop) {
                                let mut message = format!(
                                    "validator '{}': unknown property 'params.{}'",
                                    vname, prop
                                );
                                if let Some(s) = closest(prop, props.iter().copied()) {
                                    message.push_str(
                                        &format!(" - did you mean 'params.{}'?", s),
                                    );
                                }
                                errors.push(Diagnostic { code: UNKNOWN_VALIDATOR_PROPERTY, help: None, secondary: None, severity_override: None,
                                    message,
                                    context: vctx.clone(),
                                    span: None,
                                });
                            }
                        }
                    }
                    other => errors.push(Diagnostic { code: INVALID_VALIDATOR_REFERENCE, help: None, secondary: None, severity_override: None,
                        message: format!(
                            "validator '{}': `{}` is not a valid reference in `validate` \
                             (use `value.*` or `params.*`)",
                            vname, other
                        ),
                        context: vctx.clone(),
                        span: None,
                    }),
                }
            }
        }
    }

    for unit in units {
        let (owner_kind, owner_name, fields) = match unit {
            FrozenUnit::Struct { name, fields, .. } => ("Struct", name, fields),
            FrozenUnit::Error { name, fields, .. } => ("Error", name, fields),
            _ => continue,
        };
        for field in fields {
            let FrozenUnit::Field { name: field_name, parameters, span, .. } = field else {
                continue;
            };
            for param in parameters {
                let FrozenUnit::ValidatorRef { name: vname, args } = param else {
                    continue;
                };

                // Imports are keyed by their full `a::b::Name` path (or a
                // `a::b::*` glob); the other schema isn't loaded here, so any
                // import that could supply `vname` means we can't judge it.
                let maybe_imported = symbols.symbols.iter().any(|(k, v)| {
                    v.kind == SymbolType::Import
                        && (*k == vname
                            || k.ends_with(&format!("::{vname}"))
                            || k.ends_with("::*"))
                });

                let field_ctx =
                    format!("{} '{}', field '{}'", owner_kind, owner_name, field_name);

                match symbols.get(vname.as_str()) {
                    Some(SymbolType::Validator) => {
                        // Locally declared - check the keyword arguments.
                        if let Some(props) = validator_props.get(vname.as_str()) {
                            let mut seen: Vec<&str> = Vec::new();
                            for arg in args {
                                let FrozenUnit::Property { name: kw, .. } = arg else {
                                    continue;
                                };
                                if seen.contains(&kw.as_str()) {
                                    errors.push(Diagnostic { code: DUPLICATE_VALIDATOR_ARGUMENT, help: None, secondary: None, severity_override: None,
                                        message: format!(
                                            "duplicate argument '{}' to validator '{}'",
                                            kw, vname
                                        ),
                                        context: field_ctx.clone(),
                                        span: Some(*span),
                                    });
                                    continue;
                                }
                                seen.push(kw.as_str());
                                if !props.contains(&kw.as_str()) {
                                    let mut message = format!(
                                        "validator '{}' has no argument '{}'",
                                        vname, kw
                                    );
                                    if let Some(s) = closest(kw, props.iter().copied()) {
                                        message.push_str(
                                            &format!(" - did you mean '{}'?", s),
                                        );
                                    }
                                    errors.push(Diagnostic { code: UNKNOWN_VALIDATOR_ARGUMENT, help: None, secondary: None, severity_override: None,
                                        message,
                                        context: field_ctx.clone(),
                                        span: Some(*span),
                                    });
                                    continue;
                                }

                                // The argument's value against the
                                // property's declared type. Both sides are
                                // typed now: the value keeps the form it
                                // was written in, and the declared type
                                // maps onto the same vocabulary - so this
                                // no longer re-parses rendered text to
                                // guess a category.
                                let FrozenUnit::Property { value: val, .. } = arg else {
                                    continue;
                                };
                                let want = validator_fields
                                    .get(vname.as_str())
                                    .and_then(|fs| prop_field(fs, kw))
                                    .and_then(|f| match f {
                                        FrozenUnit::Field { ty, .. } => expected_value_type(ty),
                                        _ => None,
                                    });
                                if let Some(want) = want {
                                    if let Some(mismatch) = argument_mismatch(&want, val) {
                                        errors.push(Diagnostic { code: VALIDATOR_ARGUMENT_TYPE_MISMATCH, help: None, secondary: None, severity_override: None,
                                            message: format!(
                                                "validator '{}': argument '{}' expects {}, got {}",
                                                vname, kw, mismatch.expected, mismatch.found
                                            ),
                                            context: field_ctx.clone(),
                                            span: Some(*span),
                                        });
                                    }
                                }
                            }
                        }
                    }
                    Some(other) if other != SymbolType::Import => {
                        errors.push(Diagnostic { code: NOT_A_VALIDATOR, help: None, secondary: None, severity_override: None,
                            message: format!("'{}' is not a validator", vname),
                            context: format!(
                                "{} '{}', field '{}'",
                                owner_kind, owner_name, field_name
                            ),
                            span: Some(*span),
                        });
                    }
                    _ if maybe_imported => {}
                    _ => {
                        let mut message = format!("Unknown validator '{}'", vname);
                        if let Some(s) = suggest_similar_name(vname, &symbols) {
                            message.push_str(&format!(" - did you mean '{}'?", s));
                        }
                        errors.push(Diagnostic { code: UNKNOWN_VALIDATOR, help: None, secondary: None, severity_override: None,
                            message,
                            context: format!(
                                "{} '{}', field '{}'",
                                owner_kind, owner_name, field_name
                            ),
                            span: Some(*span),
                        });
                    }
                }
            }
        }
    }

    check_validator_targets(units, &symbols, imports, &mut errors);

    // Pass 3: Cycle Detection (Structs)
    let unit_map: HashMap<&str, &FrozenUnit> = units.iter().filter_map(|u| match u {
        FrozenUnit::Struct { name, .. } => Some((name.as_str(), u)),
        _ => None
    }).collect();

    let mut visited = HashSet::new();
    let mut visiting = HashSet::new();

    for (name, _) in &unit_map {
        if !visited.contains(name) {
            detect_cycle(name, &unit_map, &mut visited, &mut visiting, &mut errors);
        }
    }

    check_annotation_values(units, &mut errors);
    validate_throws(units, &mut errors);

    if errors.is_empty() {
        Ok(warnings)
    } else {
        Err(errors)
    }
}

/// Every validator names the type(s) it applies to (`for ...`), its
/// `value.*` accessors exist on those types, and every field it's applied
/// to has one of them. Matching is exact after resolving aliases (local,
/// or imported through `imports`): a `u8` field doesn't take a `for u32`
/// validator, and a validator on `string[]` checks the array, not each
/// element. `optional` is already outside the field's frozen type.
fn check_validator_targets(
    units: &[FrozenUnit],
    symbols: &SymbolTable,
    imports: &dyn ImportLookup,
    errors: &mut Vec<Diagnostic>,
) {
    let local_aliases: HashMap<&str, &TypeRef> = units
        .iter()
        .filter_map(|u| match u {
            FrozenUnit::TypeAlias { name, target, .. } => Some((name.as_str(), target)),
            _ => None,
        })
        .collect();
    let resolve = |written: &TypeRef| resolve_type(written, &local_aliases, symbols, imports);

    // `(as written, resolved)` per target, for each local validator that has
    // any.
    let mut local_targets: HashMap<&str, Vec<(TypeRef, TypeRef)>> = HashMap::new();
    for unit in units {
        let FrozenUnit::Validator { name, targets, expression_block, span, .. } = unit else {
            continue;
        };
        if targets.is_empty() {
            errors.push(
                Diagnostic::new(
                    VALIDATOR_MISSING_TARGET,
                    format!("validator '{name}' must name the type it checks"),
                )
                .with_context(format!("Validator '{name}'"))
                .with_span(*span)
                .with_help(format!(
                    "add `for` and the type after its name, e.g. `validator {name} for string`"
                )),
            );
            continue;
        }
        let pairs: Vec<(TypeRef, TypeRef)> =
            targets.iter().map(|written| (written.clone(), resolve(written))).collect();

        if let FrozenUnit::ExpressionBlock { asserts } = &**expression_block {
            let mut reported = HashSet::new();
            let references = asserts.iter().flat_map(|a| match a {
                FrozenUnit::Assert { references, .. } => references.as_slice(),
                _ => &[],
            });
            for path in references {
                let mut segs = path.split('.');
                if segs.next() != Some("value") {
                    continue;
                }
                match segs.next() {
                    None | Some("name") => {}
                    Some("length") => {
                        for (written, resolved) in &pairs {
                            if !has_length(resolved) && reported.insert(format!("length {written}")) {
                                errors.push(
                                    Diagnostic::new(
                                        VALIDATOR_ACCESSOR_UNSUPPORTED,
                                        format!(
                                            "validator '{name}': `value.length` is not defined for `{written}`"
                                        ),
                                    )
                                    .with_context(format!("Validator '{name}'"))
                                    .with_span(*span)
                                    .with_help("`value.length` exists for `string` and arrays"),
                                );
                            }
                        }
                    }
                    Some(other) => {
                        if reported.insert(other.to_string()) {
                            errors.push(
                                Diagnostic::new(
                                    VALIDATOR_ACCESSOR_UNSUPPORTED,
                                    format!("validator '{name}': `value` has no accessor `{other}`"),
                                )
                                .with_context(format!("Validator '{name}'"))
                                .with_span(*span)
                                .with_help(
                                    "available: `value`, `value.length` (`string` and arrays), `value.name`",
                                ),
                            );
                        }
                    }
                }
            }
        }
        local_targets.insert(name.as_str(), pairs);
    }

    for unit in units {
        let (owner_kind, owner_name, fields) = match unit {
            FrozenUnit::Struct { name, fields, .. } => ("Struct", name, fields),
            FrozenUnit::Error { name, fields, .. } => ("Error", name, fields),
            _ => continue,
        };
        for field in fields {
            let FrozenUnit::Field { name: field_name, parameters, ty: field_written, span, .. } =
                field
            else {
                continue;
            };
            let field_resolved = resolve(field_written);

            for param in parameters {
                let FrozenUnit::ValidatorRef { name: vname, .. } = param else {
                    continue;
                };
                // A local validator without targets was reported above, and
                // an unknown name by pass 2.5 - neither has anything to match.
                let targets: Vec<(TypeRef, TypeRef)> = match local_targets.get(vname.as_str()) {
                    Some(pairs) => pairs.clone(),
                    None if matches!(symbols.get(vname), None | Some(SymbolType::Import)) => {
                        match imports.validator_targets(vname) {
                            Some(t) => t.into_iter().map(|w| (w.clone(), w)).collect(),
                            None => continue,
                        }
                    }
                    None => continue,
                };
                if targets.is_empty() || targets.iter().any(|(_, r)| *r == field_resolved) {
                    continue;
                }
                let applies_to = targets
                    .iter()
                    .map(|(w, _)| format!("`{w}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut found = format!("`{field_written}`");
                if field_resolved != *field_written {
                    found.push_str(&format!(" (`{field_resolved}`)"));
                }
                errors.push(
                    Diagnostic::new(
                        VALIDATOR_TARGET_MISMATCH,
                        format!("validator '{vname}' applies to {applies_to}, not {found}"),
                    )
                    .with_context(format!("{owner_kind} '{owner_name}', field '{field_name}'"))
                    .with_span(*span),
                );
            }
        }
    }
}

/// `ty` with every alias replaced by what it stands for, all the way down:
/// `type Name = string` makes `Name[]` resolve to `string[]`. Local aliases
/// first; a name this schema doesn't declare goes to `imports`. Bounded, so
/// a cycle (already reported by alias resolution) can't hang it.
fn resolve_type(
    ty: &TypeRef,
    local_aliases: &HashMap<&str, &TypeRef>,
    symbols: &SymbolTable,
    imports: &dyn ImportLookup,
) -> TypeRef {
    fn go(
        ty: &TypeRef,
        local_aliases: &HashMap<&str, &TypeRef>,
        symbols: &SymbolTable,
        imports: &dyn ImportLookup,
        depth: usize,
    ) -> TypeRef {
        if depth == 32 {
            return ty.clone();
        }
        ty.map_named(&mut |name| match local_aliases.get(name) {
            Some(target) => Some(go(target, local_aliases, symbols, imports, depth + 1)),
            None if matches!(symbols.get(name), None | Some(SymbolType::Import)) => {
                imports.alias_target(name)
            }
            None => None,
        })
    }
    go(ty, local_aliases, symbols, imports, 0)
}

/// Whether `value.length` means something for this (resolved) type.
fn has_length(resolved: &TypeRef) -> bool {
    resolved.is_array() || *resolved == TypeRef::Primitive(PrimitiveType::String)
}

/// Check every *known* annotation's value against the shape its registry
/// entry declares. Core had no opinion on these at all before: both
/// `@timeout_ms = "abc"` and `@framing = "nonsense"` compiled clean and
/// were then silently dropped by whichever generator tried to read them.
///
/// Only keys in [`annotations::KNOWN_ANNOTATIONS`] are checked - `@key` is
/// an open namespace by design, so an unrecognised one is not an error and
/// has no declared shape to check against.
fn check_annotation_values(units: &[FrozenUnit], errors: &mut Vec<Diagnostic>) {
    for unit in units {
        match unit {
            FrozenUnit::Struct { name, parameters, fields, span, .. } => {
                check_params(parameters, &format!("struct '{name}'"), Some(*span), errors);
                check_field_params(fields, errors);
            }
            FrozenUnit::Error { name, parameters, fields, span, .. } => {
                check_params(parameters, &format!("error '{name}'"), *span, errors);
                check_field_params(fields, errors);
            }
            FrozenUnit::Protocol { name, parameters, functions, span, .. } => {
                check_params(parameters, &format!("protocol '{name}'"), Some(*span), errors);
                for function in functions {
                    if let FrozenUnit::Function { name: fname, parameters, span, .. } = function {
                        check_params(
                            parameters,
                            &format!("protocol '{name}', function '{fname}'"),
                            Some(*span),
                            errors,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

fn check_field_params(fields: &[FrozenUnit], errors: &mut Vec<Diagnostic>) {
    for field in fields {
        if let FrozenUnit::Field { name, parameters, span, .. } = field {
            check_params(parameters, &format!("field '{name}'"), Some(*span), errors);
        }
    }
}

fn check_params(
    parameters: &[FrozenUnit],
    context: &str,
    span: Option<(usize, usize)>,
    errors: &mut Vec<Diagnostic>,
) {
    for param in parameters {
        let FrozenUnit::Property { name, value } = param else {
            continue;
        };
        let Some(info) = annotations::lookup(name) else {
            continue;
        };
        if let Err(mismatch) = config_value::check(&info.value, value.form()) {
            errors.push(
                Diagnostic::new(
                    ANNOTATION_VALUE_MISMATCH,
                    format!("'@{name}' must be {}, got {}", mismatch.expected, mismatch.found),
                )
                .with_context(context.to_string())
                .with_help(format!("'@{name}' takes {}", mismatch.expected))
                .maybe_span(span),
            );
        }
    }
}

fn detect_cycle<'a>(
    current: &'a str, 
    unit_map: &HashMap<&'a str, &'a FrozenUnit>, 
    visited: &mut HashSet<&'a str>, 
    visiting: &mut HashSet<&'a str>,
    errors: &mut Vec<Diagnostic>
) {
    visiting.insert(current);
    
    if let Some(FrozenUnit::Struct { fields, span, .. }) = unit_map.get(current) {
        for field in fields {
            if let FrozenUnit::Field { ty, .. } = field {
                // Only a dynamic array breaks a cycle - it can be empty. A
                // fixed array `Node[4]` always holds four, so a struct
                // containing one of itself is still infinite.
                if !breaks_cycle(ty) {
                    let Some(base_type) = ty.base_name() else { continue };

                    if unit_map.contains_key(base_type) {
                        if visiting.contains(base_type) {
                            errors.push(Diagnostic { code: STRUCT_CYCLE, help: None, secondary: None, severity_override: None,
                                message: format!("Cycle detected involving struct '{}'", base_type),
                                context: format!("Struct '{}' depends on '{}'", current, base_type),
                                span: Some(*span),
                            });
                        } else if !visited.contains(base_type) {
                            detect_cycle(base_type, unit_map, visited, visiting, errors);
                        }
                    }
                }
            }
        }
    }

    visiting.remove(current);
    visited.insert(current);
}

/// Look for a `deprecated` property among a struct/error's own annotation
/// parameters. Outer `Option`: is it deprecated at all - `None` for absent
/// or explicit `@deprecated = false`. Inner: an optional message - a bare
/// marker or `= true` means "deprecated, no message"; `= "message"` carries
/// it. Anything else (a bare word, a `::`-path, an integer, an f-string, a
/// list) is a real `Err`, not silently treated as a message - mirrors Rust's
/// own `#[deprecated]`/`#[deprecated = "note"]`, which only ever accepts a
/// bare marker or a quoted string, never an arbitrary bare identifier.
fn deprecation_of(
    parameters: &[FrozenUnit],
    span: Option<(usize, usize)>,
) -> Result<Option<Option<String>>, Diagnostic> {
    for p in parameters {
        let FrozenUnit::Deprecated(value) = p else { continue };
        return match value {
            DeprecatedValue::False => Ok(None),
            DeprecatedValue::True => Ok(Some(None)),
            DeprecatedValue::Message(msg) => Ok(Some(Some(msg.clone()))),
            DeprecatedValue::Malformed(text) => Err(Diagnostic::new(
                DEPRECATED_MALFORMED_VALUE,
                format!("'@deprecated' must be a boolean or a string message, got '{text}'"),
            )
            .with_help(
                "accepted values: a bare `@deprecated` marker, `= true`, `= false`, or `= \"message\"`",
            )
            .maybe_span(span)),
        };
    }
    Ok(None)
}

/// Whether a field of this type can stand in a struct that contains
/// itself: only through a dynamic array, which can be empty.
fn breaks_cycle(ty: &TypeRef) -> bool {
    matches!(ty, TypeRef::Array { length: None, .. })
}

/// A literal written for a value of type `ty` - a field or property
/// default, a constant - has to be that type, and an integer has to fit
/// the width declared. A reference (`MAX`, `u32::MAX`) isn't resolved yet,
/// so it isn't judged; nor is a literal for a non-primitive type.
fn check_literal(
    ty: &TypeRef,
    literal: &Literal,
    context: &str,
    span: (usize, usize),
    errors: &mut Vec<Diagnostic>,
) {
    let Some(primitive) = ty.as_primitive() else { return };
    let fits = match (primitive, literal) {
        (_, Literal::Reference(_)) => return,
        (p, Literal::Integer(value)) if p.is_integer() => {
            let (min, max) = p.integer_range().expect("an integer primitive has a range");
            if !(min..=max).contains(&i128::from(*value)) {
                errors.push(
                    Diagnostic::new(
                        VALUE_OUT_OF_RANGE,
                        format!("the value {value} does not fit in `{primitive}`"),
                    )
                    .with_context(context.to_string())
                    .with_help(format!(
                        "either change the value to one `{primitive}` can hold, or widen the \
                         declared type"
                    ))
                    .with_span(span),
                );
            }
            return;
        }
        // An integer literal is a fine value for a float.
        (p, Literal::Integer(_)) => p.is_float(),
        (PrimitiveType::Bool, Literal::Bool(_)) => true,
        (PrimitiveType::String, Literal::String(_)) => true,
        _ => false,
    };
    if !fits {
        errors.push(
            Diagnostic::new(
                LITERAL_TYPE_MISMATCH,
                format!("expected a `{primitive}` value, found {} `{literal}`", literal.describe()),
            )
            .with_context(context.to_string())
            .with_span(span),
        );
    }
}

/// Check every function's `! Name` clause.
///
/// Freeze resolves a throw to a schema-global ordinal before `validate` runs,
/// so a bad name is not a name here any more - it is a placeholder
/// [`FrozenUnit::Error`] carrying a marker in `imported_from`, the same
/// encode-in-the-IR convention. This is where those
/// become messages, reported at the throwing function's span rather than the
/// placeholder's (a re-export has no span of its own).
fn validate_throws(units: &[FrozenUnit], errors: &mut Vec<Diagnostic>) {
    // ordinal -> (name, `imported_from` marker if it is a placeholder)
    let mut slots: HashMap<u16, (&str, Option<&str>)> = HashMap::new();
    let mut real_errors: Vec<&str> = vec![];
    for unit in units {
        if let FrozenUnit::Error { ordinal, name, imported_from, .. } = unit {
            let marker = imported_from.as_deref().filter(|from| {
                from.starts_with(UNRESOLVED_PREFIX) || from.starts_with(NOT_AN_ERROR_PREFIX)
            });
            if marker.is_none() {
                real_errors.push(name);
            }
            slots.insert(*ordinal, (name, marker));
        }
    }

    for unit in units {
        let FrozenUnit::Protocol { name: proto, functions, .. } = unit else {
            continue;
        };
        for function in functions {
            let FrozenUnit::Function { name: func, throws, span, .. } = function else {
                continue;
            };
            let context = format!("Protocol '{proto}', function '{func}'");

            let mut seen: HashSet<u16> = HashSet::new();
            for ordinal in throws {
                let Some((name, marker)) = slots.get(ordinal).copied() else {
                    continue;
                };

                if !seen.insert(*ordinal) {
                    errors.push(
                        Diagnostic::new(
                            DUPLICATE_THROW,
                            format!("function '{func}' raises '{name}' more than once"),
                        )
                        .with_context(context.clone())
                        .with_help("list each error once - `! A, B`, not `! A, A`")
                        .with_span(*span),
                    );
                    continue;
                }

                let Some(marker) = marker else { continue };

                if let Some(rest) = marker
                    .strip_prefix(NOT_AN_ERROR_PREFIX)
                    .and_then(|r| r.strip_suffix('>'))
                {
                    let kind = rest
                        .split_once(": ")
                        .map(|(_, k)| k)
                        .unwrap_or("another declaration");
                    errors.push(
                        Diagnostic::new(
                            THROW_NOT_AN_ERROR,
                            format!("'{name}' is {kind}, so it cannot be raised"),
                        )
                        .with_context(context.clone())
                        .with_help(
                            "only an `error` declaration can follow `!` - it is what carries \
                             the `message` and the wire ordinal a raised failure travels as",
                        )
                        .with_span(*span),
                    );
                    continue;
                }

                let mut message = format!("no error named '{name}' is in scope");
                if let Some(suggestion) = closest(name, real_errors.iter().copied()) {
                    message.push_str(&format!(" - did you mean '{suggestion}'?"));
                }
                errors.push(
                    Diagnostic::new(UNRESOLVED_THROW, message)
                        .with_context(context.clone())
                        .with_help(
                            "declare it with `error <Name> { message = \"...\" }`, or `use` the \
                             schema that declares it",
                        )
                        .with_span(*span),
                );
            }
        }
    }
}

fn validate_type(
    ty: &TypeRef, symbols: &SymbolTable, deprecated: &HashMap<&str, Option<String>>,
    errors: &mut Vec<Diagnostic>, warnings: &mut Vec<Diagnostic>, context: &str, span: (usize, usize),
) {
    match ty {
        TypeRef::Named(name) => {
            // Resolve the name, then check that what it names can actually
            // stand here. Presence used to be the whole test, so anything in
            // the table passed: `field: SomeProtocol`, `field: SOME_CONST`
            // and `field: SomeValidator` all type-checked, and adding errors
            // to the table would have quietly extended that to them.
            //
            // A name the bare-import set brought into scope is opaque - the
            // schema it came from is not necessarily parsed here - so it is
            // accepted without a kind.
            let found = symbols.get(name);
            let imported_bare = symbols.is_imported_bare(name);

            if let Some(kind) = found.filter(|k| !k.usable_as_type()) {
                errors.push(
                    Diagnostic::new(
                        NOT_A_TYPE,
                        format!("'{name}' is {}, not a type", kind.describe()),
                    )
                    .with_context(context.to_string())
                    .with_span(span),
                );
            } else if found.is_none() && !imported_bare {
                // A text-type hint says exactly what to write; a "did you
                // mean" alongside it would only contradict it (`str` is one
                // edit from `s8`).
                let hint = text_type_hint(name);
                let mut message = format!("Unknown type '{}'", name);
                if hint.is_none() {
                    if let Some(suggestion) = suggest_similar_name(name, symbols) {
                        message.push_str(&format!(" - did you mean '{}'?", suggestion));
                    }
                }
                errors.push(Diagnostic { code: UNKNOWN_TYPE, help: hint, secondary: None, severity_override: None,
                    message,
                    context: context.to_string(),
                    span: Some(span),
                });
            } else if let Some(deprecation_message) = deprecated.get(name.as_str()) {
                let message = match deprecation_message {
                    Some(msg) => format!("'{}' is deprecated: {}", name, msg),
                    None => format!("'{}' is deprecated", name),
                };
                warnings.push(Diagnostic::new(DEPRECATED_TYPE_USAGE, message)
                    .with_context(context.to_string())
                    .with_span(span));
            }
        }
        TypeRef::Array { element, .. } => {
            validate_type(element, symbols, deprecated, errors, warnings, context, span);
        }
        TypeRef::Union(members) => {
            for member in members {
                validate_type(member, symbols, deprecated, errors, warnings, context, span);
            }
        }
        // Primitives and `()` have nothing to resolve.
        TypeRef::Primitive(_) | TypeRef::Unit => {}
    }
}

/// Every primitive's name: the grammar's own, plus `u128`/`s128` (see
/// [`PrimitiveType`]).
fn primitive_names() -> impl Iterator<Item = &'static str> + Clone {
    PrimitiveType::ALL.into_iter().map(PrimitiveType::name)
}

pub(crate) fn is_primitive(name: &str) -> bool {
    primitive_names().any(|n| n == name)
}

/// A validator's declared property names.
fn prop_names(props: &[FrozenUnit]) -> Vec<&str> {
    props
        .iter()
        .filter_map(|p| match p {
            FrozenUnit::Field { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect()
}

/// The property `FrozenUnit::Field` with the given name.
fn prop_field<'a>(props: &'a [FrozenUnit], name: &str) -> Option<&'a FrozenUnit> {
    props
        .iter()
        .find(|p| matches!(p, FrozenUnit::Field { name: n, .. } if n == name))
}

/// Whether a validator-call argument's value conflicts with the property's
/// declared type.
///
/// A bare name or a `::`-path is a *reference* to something declared
/// elsewhere (`Bounds(min = MIN)`), and this pass does not resolve those -
/// so there is nothing to judge and no error to report. That has always
/// been the behaviour; it used to fall out of the old text-sniffing
/// returning "no category" for anything that wasn't an obvious literal,
/// which quietly conflated "a reference" with "a type I don't handle".
/// Stating it as its own case makes the intent explicit.
fn argument_mismatch(
    want: &ValueType,
    val: &ConfigValue,
) -> Option<crate::config_value::TypeMismatch> {
    match val {
        ConfigValue::Name(_) | ConfigValue::Path(_) | ConfigValue::Marker => None,
        _ => config_value::check(want, val.form()).err(),
    }
}

/// What a validator property's declared type accepts, in the shared
/// configuration-value vocabulary.
///
/// The argument keeps its written form, and this maps the declared type
/// straight onto the same vocabulary that checks it - with or without a
/// default, since the type no longer depends on one being written.
///
/// `None` for a type this cannot express - a custom/named type, a float, an
/// array, a union - leaving the argument unchecked rather than guessing.
fn expected_value_type(ty: &TypeRef) -> Option<ValueType> {
    match ty.as_primitive()? {
        PrimitiveType::Bool => Some(ValueType::Bool),
        PrimitiveType::String => Some(ValueType::Str),
        p if p.is_integer() => Some(ValueType::Integer),
        _ => None,
    }
}

/// The text type is spelled `string`. `str` was an accepted spelling until
/// it was removed outright (no deprecation period - pre-1.0), and `String`
/// is what the type generates to in Rust; both now parse as an ordinary
/// unknown name, so say what to write instead.
fn text_type_hint(type_name: &str) -> Option<String> {
    match type_name {
        "str" => Some("`str` was removed; use `string`".to_string()),
        "String" => Some("the text type is spelled `string`".to_string()),
        _ => None,
    }
}

/// Find the closest known symbol or primitive name to an unrecognized
/// type reference, for a "did you mean 'X'?" suggestion - `None` if
/// nothing is close enough to plausibly be a typo rather than just an
/// unrelated, genuinely nonexistent name.
fn suggest_similar_name(target: &str, symbols: &SymbolTable) -> Option<String> {
    // Allow up to half the target's length to differ (rounded up, at
    // least 1). A single transposition - swapping two adjacent letters,
    // e.g. "Uesr" -> "User" - already costs 2 under plain Levenshtein
    // (no dedicated swap operation), so a stricter ratio like 1/3 would
    // reject that exact typo; half the length comfortably covers it
    // while still excluding wildly different names.
    let len = target.chars().count();
    let max_distance = ((len + 1) / 2).max(1);

    // Collected (not chained directly): `primitive_names()`'s `impl
    // Iterator<Item = &'static str>` won't unify with `symbols`'s shorter-
    // lived `&str` under `chain`'s exact-type-match bound without this
    // intermediate step, even though `'static` trivially satisfies it.
    let primitives: Vec<&str> = primitive_names().collect();

    symbols
        .symbols
        .keys()
        .copied()
        .chain(primitives)
        .filter(|&name| name != target)
        .map(|name| (name, levenshtein_distance(target, name)))
        .filter(|&(_, distance)| distance <= max_distance)
        .min_by_key(|&(_, distance)| distance)
        .map(|(name, _)| name.to_string())
}
