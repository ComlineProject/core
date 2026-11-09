// Standard Uses
use std::collections::{HashMap, HashSet};

// Crate Uses
use crate::diagnostics::{Diagnostic, DiagnosticCode};
use super::symbols::{SymbolTable, SymbolType};
use crate::schema::ir::frozen::unit::{schema_namespace, ConfigValue, DeprecatedValue, FrozenUnit};
use crate::schema::ir::compiler::interpreter::incremental::{NOT_AN_ERROR_PREFIX, UNRESOLVED_PREFIX};
use crate::schema::ir::frozen::types::{Literal, PrimitiveType, TypeRef};
use super::constants::{Constants, Problem, Resolution, ResolvedConstant};
use crate::utils::text::{closest, levenshtein_distance};
use crate::config_value::{self, ValueType};
use crate::schema::idl::annotations;

const DUPLICATE_DECLARATION: DiagnosticCode = DiagnosticCode::error("validation::duplicate-declaration");
const INVALID_CONSTANT_TYPE: DiagnosticCode = DiagnosticCode::error("validation::invalid-constant-type");
const VALUE_OUT_OF_RANGE: DiagnosticCode =
    DiagnosticCode::error("validation::value-out-of-range");
const LITERAL_TYPE_MISMATCH: DiagnosticCode =
    DiagnosticCode::error("validation::literal-type-mismatch");
const UNION_MEMBERS_DIFFER_ONLY_BY_BOUND: DiagnosticCode =
    DiagnosticCode::error("validation::union-members-differ-only-by-bound");
const UNKNOWN_CONSTANT: DiagnosticCode = DiagnosticCode::error("validation::unknown-constant");
const NOT_A_CONSTANT: DiagnosticCode = DiagnosticCode::error("validation::not-a-constant");
const CONSTANT_CYCLE: DiagnosticCode = DiagnosticCode::error("validation::constant-cycle");
const UNKNOWN_VALIDATOR_PROPERTY: DiagnosticCode =
    DiagnosticCode::error("validation::unknown-validator-property");
const INVALID_VALIDATOR_REFERENCE: DiagnosticCode =
    DiagnosticCode::error("validation::invalid-validator-reference");
const DUPLICATE_VALIDATOR_ARGUMENT: DiagnosticCode =
    DiagnosticCode::error("validation::duplicate-validator-argument");
const UNKNOWN_VALIDATOR_ARGUMENT: DiagnosticCode =
    DiagnosticCode::error("validation::unknown-validator-argument");
const MISSING_VALIDATOR_ARGUMENT: DiagnosticCode =
    DiagnosticCode::error("validation::missing-validator-argument");
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
const DUPLICATE_FIELD: DiagnosticCode = DiagnosticCode::error("validation::duplicate-field");
const DUPLICATE_VARIANT: DiagnosticCode = DiagnosticCode::error("validation::duplicate-variant");
const DUPLICATE_FUNCTION: DiagnosticCode = DiagnosticCode::error("validation::duplicate-function");
const DUPLICATE_ARGUMENT: DiagnosticCode = DiagnosticCode::error("validation::duplicate-argument");
const DUPLICATE_PARAMETER: DiagnosticCode = DiagnosticCode::error("validation::duplicate-parameter");
const MISPLACED_VALIDATORS: DiagnosticCode =
    DiagnosticCode::error("validation::misplaced-validators");
const VALIDATOR_MISSING_TARGET: DiagnosticCode =
    DiagnosticCode::error("validation::validator-missing-target");
const VALIDATOR_TARGET_MISMATCH: DiagnosticCode =
    DiagnosticCode::error("validation::validator-target-mismatch");
const VALIDATOR_ANY_MISPLACED: DiagnosticCode =
    DiagnosticCode::error("validation::validator-any-misplaced");
const VALIDATOR_MINIMUM_ABOVE_MAXIMUM: DiagnosticCode =
    DiagnosticCode::error("bounds::validator-minimum-above-maximum");
const VALIDATOR_CAPS_BELOW_TYPE: DiagnosticCode =
    DiagnosticCode::warning("bounds::validator-caps-below-type");
const VALIDATOR_ACCESSOR_UNSUPPORTED: DiagnosticCode =
    DiagnosticCode::error("validation::validator-accessor-unsupported");
const VALIDATOR_CHECKS_NOTHING: DiagnosticCode =
    DiagnosticCode::warning("validation::validator-checks-nothing");
const VALIDATOR_PROPERTY_UNUSED: DiagnosticCode =
    DiagnosticCode::warning("validation::validator-property-unused");
const MESSAGE_NOT_INTERPOLATED: DiagnosticCode =
    DiagnosticCode::warning("validation::message-not-interpolated");
const PROVIDER_CONSUMER_MISMATCH: DiagnosticCode =
    DiagnosticCode::error("validation::provider-consumer-mismatch");
const FORMAT_DOESNT_FIT_FRAMING: DiagnosticCode =
    DiagnosticCode::error("validation::format-doesnt-fit-framing");
const REVERSE_CALLS_NEED_FRAMING: DiagnosticCode =
    DiagnosticCode::error("validation::reverse-calls-need-framing");
const UNVERIFIED_MISSING_MARKER: DiagnosticCode =
    DiagnosticCode::error("unverified::missing-marker");
const UNVERIFIED_MISSING_REASON: DiagnosticCode =
    DiagnosticCode::error("unverified::missing-reason");
const UNVERIFIED_UNKNOWN_GUARANTEE: DiagnosticCode =
    DiagnosticCode::error("unverified::unknown-guarantee");
const UNVERIFIED_DUPLICATE_GUARANTEE: DiagnosticCode =
    DiagnosticCode::error("unverified::duplicate-guarantee");
const UNVERIFIED_MISSING_GUARANTEE: DiagnosticCode =
    DiagnosticCode::error("unverified::missing-guarantee");
const UNVERIFIED_NOT_GIVEN_UP: DiagnosticCode =
    DiagnosticCode::warning("unverified::not-given-up");
const UNVERIFIED_NOTHING_GIVEN_UP: DiagnosticCode =
    DiagnosticCode::warning("unverified::nothing-given-up");
const UNVERIFIED_SCHEMA_AGREEMENT: DiagnosticCode =
    DiagnosticCode::warning("unverified::schema-agreement");
const UNVERIFIED_ENCODING_AGREEMENT: DiagnosticCode =
    DiagnosticCode::warning("unverified::encoding-agreement");
const UNVERIFIED_ENCODING_CONFORMANCE: DiagnosticCode =
    DiagnosticCode::warning("unverified::encoding-conformance");
const UNVERIFIED_BOUNDED_SIZE: DiagnosticCode =
    DiagnosticCode::warning("unverified::bounded-size");
const UNVERIFIED_VERSION_HONESTY: DiagnosticCode =
    DiagnosticCode::warning("unverified::version-honesty");

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
    /// An imported validator's properties, frozen the way its own schema
    /// freezes them (`FrozenUnit::Field`s), for checking a call's arguments.
    /// `None` when the lookup can't read the declaring schema: the arguments
    /// then go unchecked.
    fn validator_properties(&self, _name: &str) -> Option<Vec<FrozenUnit>> {
        None
    }
    /// Whether `name`'s import is only a stand-in: an editor that reports a
    /// missing `use` itself adds an import for the name, so core doesn't also
    /// call it an unknown type - but nothing is in scope behind it, and no
    /// check should be judged against it. `comline build` has none.
    fn is_stand_in(&self, _name: &str) -> bool {
        false
    }
    /// Whether an imported type puts any bytes on the wire (see
    /// `validation::data`), worked out in the schema that declares it.
    /// `None` when the lookup can't say: the type is then taken to carry
    /// data, and an array of it goes unchecked.
    fn carries_data(&self, _name: &str) -> Option<bool> {
        None
    }
    /// Whether the package, or one of its dependencies, declares `name` as
    /// an encoding of `kind` under `encodings` in its `config.idp`. `false`
    /// when the lookup can't say: then only the official names are known.
    fn encoding_declared(&self, _kind: crate::encodings::EncodingKind, _name: &str) -> bool {
        false
    }
    /// The encoding declarations and package wire defaults the guarantee
    /// check reads (`unverified::given_up`). Without a package, only the
    /// official encodings.
    fn encodings(&self) -> crate::schema::ir::unverified::Encodings<'_> {
        Default::default()
    }
    /// An imported constant's type and value, resolved in the schema that
    /// declares it (core#168). `None` when the lookup can't say, or the name
    /// isn't a constant there: a value that names it then goes unchecked.
    fn constant(&self, _name: &str) -> Option<ResolvedConstant> {
        None
    }
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

        // Deprecation is tracked for structs and type aliases - the kinds
        // whose *usage site* is a type position this pass reaches. A
        // function's `! Name` throws is already an ordinal by the time
        // `validate` runs (`interpreter::incremental` resolves it), so
        // `@deprecated` on an error, a protocol, a field, or a function
        // parses fine and has nothing for this check to flag.
        if let FrozenUnit::Struct { parameters, .. } | FrozenUnit::TypeAlias { parameters, .. } =
            unit
        {
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

    // What a value can name: this schema's constants, the ones it imports,
    // and an integer type's bounds. And its aliases, for the primitive a
    // default is written for.
    let constants = Constants::new(units, namespace, &symbols, imports);
    let local_aliases: HashMap<&str, &TypeRef> = units
        .iter()
        .filter_map(|u| match u {
            FrozenUnit::TypeAlias { name, target, .. } => Some((name.as_str(), target)),
            _ => None,
        })
        .collect();
    // A default has to be its type's kind of value and, for a string with a
    // bound, fit it (core#98).
    let check_default = |ty: &TypeRef, default: &Literal, context: &str, span, errors: &mut Vec<Diagnostic>| {
        match resolve_type(ty, &local_aliases, &symbols, imports) {
            TypeRef::Primitive(primitive) => check_literal(primitive, default, &constants, context, span, errors),
            TypeRef::Array { element, length, max } if *element == TypeRef::Primitive(PrimitiveType::Char) => {
                let before = errors.len();
                check_literal(PrimitiveType::String, default, &constants, context, span, errors);
                if errors.len() == before {
                    check_text_size(default, length, max, &constants, context, span, errors);
                }
            }
            _ => {}
        }
    };

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
                                check_default(ty, default, &context, *span, &mut errors);
                            }
                        }
                        _ => {}
                    }
                }
            }
            // An error's fields and a validator's properties take defaults
            // the same way a struct's fields do.
            FrozenUnit::Error { name, fields, imported_from: None, .. } => {
                for field in fields {
                    if let FrozenUnit::Field { name: field_name, ty, default: Some(default), span, .. } = field {
                        let context = format!("Error '{name}', field '{field_name}'");
                        check_default(ty, default, &context, *span, &mut errors);
                    }
                }
            }
            FrozenUnit::Validator { name, properties, .. } => {
                for property in properties {
                    if let FrozenUnit::Field { name: property_name, ty, default: Some(default), span, .. } = property {
                        let context = format!("Validator '{name}', property '{property_name}'");
                        check_default(ty, default, &context, *span, &mut errors);
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
                    // A constant is a primitive with a value: a literal, or
                    // another constant (core#168). Either has to fit it; a
                    // chain that comes back here is reported here.
                    (Some(primitive), Some(Literal::Reference(reference))) => check_resolved(
                        primitive,
                        reference,
                        constants.resolve_from(name, reference),
                        &context,
                        *span,
                        &mut errors,
                    ),
                    (Some(primitive), Some(literal)) => {
                        check_literal(primitive, literal, &constants, &context, *span, &mut errors);
                    }
                    (Some(_), None) => errors.push(
                        Diagnostic::new(
                            INVALID_CONSTANT_TYPE,
                            format!("Constant '{name}' must have a literal value"),
                        )
                        .with_context(context)
                        .with_help("write a literal (`5`, `\"text\"`, `true`) or another constant's name")
                        .with_span(*span),
                    ),
                    (None, _) => errors.push(Diagnostic { code: INVALID_CONSTANT_TYPE, help: text_type_hint(&ty.to_string()), secondary: None, severity_override: None, fix: None,
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

    // Pass 2.5: every `@validators = [Name(...)]` on a struct/error field or a
    // `type` alias must
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
        let FrozenUnit::Validator { name: vname, expression_block, span: vspan, .. } = unit else {
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
                                errors.push(Diagnostic { code: UNKNOWN_VALIDATOR_PROPERTY, help: None, secondary: None, severity_override: None, fix: None,
                                    message,
                                    context: vctx.clone(),
                                    span: Some(*vspan),
                                });
                            }
                        }
                    }
                    other => errors.push(Diagnostic { code: INVALID_VALIDATOR_REFERENCE, help: None, secondary: None, severity_override: None, fix: None,
                        message: format!(
                            "validator '{}': `{}` is not a valid reference in `validate` \
                             (use `value.*` or `params.*`)",
                            vname, other
                        ),
                        context: vctx.clone(),
                        span: Some(*vspan),
                    }),
                }
            }
        }
    }

    // Pass 2.7: a validator that checks nothing, and properties nothing
    // reads. Both build, so both warn: the first passes every value, the
    // second can be set in a call and has no effect.
    for unit in units {
        let FrozenUnit::Validator {
            name, properties, targets, expression_block, span, validate_span, ..
        } = unit
        else {
            continue;
        };
        let asserts: &[FrozenUnit] = match &**expression_block {
            FrozenUnit::ExpressionBlock { asserts } => asserts,
            _ => &[],
        };
        if asserts.is_empty() {
            // Its properties are unused too, but that's the same omission:
            // one warning, not one per property.
            // An example that is valid for what it checks: `value.length`
            // only exists for a string or an array.
            let example = if !targets.is_empty() && targets.iter().all(has_length) {
                "assert(value.length > 0, f\"{value.name} is empty\")"
            } else {
                "assert(value > 0, f\"{value.name} must be positive\")"
            };
            // An empty `validate` block is where the assert goes, so that's
            // what is pointed at; without one, the validator itself.
            let diagnostic = match validate_span {
                Some(block) => Diagnostic::new(
                    VALIDATOR_CHECKS_NOTHING,
                    format!("validator '{name}' checks nothing: its `validate` block has no `assert`"),
                )
                .with_span(*block)
                .with_secondary_span(*span, format!("validator '{name}'"))
                .with_help(format!("add an assert, e.g. `{example}`")),
                None => Diagnostic::new(
                    VALIDATOR_CHECKS_NOTHING,
                    format!("validator '{name}' checks nothing: it has no `validate` block"),
                )
                .with_span(*span)
                .with_help(format!(
                    "add a `validate` block with an assert, e.g. `validate {{ {example} }}`"
                )),
            };
            warnings.push(diagnostic.with_context(format!("Validator '{name}'")));
            continue;
        }
        let used: HashSet<&str> = asserts
            .iter()
            .flat_map(|a| match a {
                FrozenUnit::Assert { references, message, .. } => references
                    .iter()
                    .map(String::as_str)
                    .chain(message_placeholders(message))
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            })
            .filter_map(|path| path.strip_prefix("params."))
            .map(|rest| rest.split('.').next().unwrap_or(rest))
            .collect();
        for property in properties {
            let FrozenUnit::Field { name: prop, span: prop_span, .. } = property else {
                continue;
            };
            if !used.contains(prop.as_str()) {
                warnings.push(
                    Diagnostic::new(
                        VALIDATOR_PROPERTY_UNUSED,
                        format!("validator '{name}': property '{prop}' is never used"),
                    )
                    .with_context(format!("Validator '{name}'"))
                    .with_span(*prop_span)
                    .with_help(format!(
                        "read it as `params.{prop}` in an assert or its message, or remove it"
                    )),
                );
            }
        }
    }

    for site in validated_sites(units) {
        let span = &site.span;
        for param in site.parameters {
            let FrozenUnit::ValidatorRef { name: vname, args, span: call_span, arg_spans } = param else {
                continue;
            };
            // The call's own name, when the lowering recorded it.
            let span = &known_or(*call_span, *span);

            // The other schema isn't loaded here, so any import that could
            // supply `vname` means we can't judge it.
            let maybe_imported = could_be_imported(&symbols, vname);

            let field_ctx = site.context.clone();

            match symbols.get(vname.as_str()) {
                Some(SymbolType::Validator) => {
                    // Locally declared - check the keyword arguments.
                    if let Some(fields) = validator_fields.get(vname.as_str()) {
                        check_validator_arguments(vname, fields, args, arg_spans, &constants, &field_ctx, *span, &mut errors);
                    }
                }
                Some(other) if other != SymbolType::Import => {
                    errors.push(Diagnostic { code: NOT_A_VALIDATOR, help: None, secondary: None, severity_override: None, fix: None,
                        message: format!("'{}' is not a validator", vname),
                        context: field_ctx.clone(),
                        span: Some(*span),
                    });
                }
                // Imported: its arguments are checked against the properties
                // the lookup can read off the declaring schema; when it can't,
                // there's nothing to judge against.
                _ if maybe_imported => {
                    if let Some(fields) = imports.validator_properties(vname) {
                        check_validator_arguments(vname, &fields, args, arg_spans, &constants, &field_ctx, *span, &mut errors);
                    }
                }
                _ => {
                    let mut message = format!("Unknown validator '{}'", vname);
                    if let Some(s) = suggest_similar_name(vname, &symbols) {
                        message.push_str(&format!(" - did you mean '{}'?", s));
                    }
                    errors.push(Diagnostic { code: UNKNOWN_VALIDATOR, help: None, secondary: None, severity_override: None, fix: None,
                        message,
                        context: field_ctx.clone(),
                        span: Some(*span),
                    });
                }
            }
    }
    }

    check_validator_targets(units, &symbols, imports, &mut errors);
    check_misplaced_validators(units, &mut errors);

    check_uninterpolated_messages(units, &mut warnings);

    // Pass 3: structs that can never hold a finite value
    check_struct_cycles(units, &mut errors);

    check_validator_sizes(units, &symbols, imports, &constants, &mut errors, &mut warnings);
    check_annotation_values(units, imports, &mut errors);
    check_unverified(units, &imports.encodings(), &mut errors, &mut warnings);
    check_provider(units, &imports.encodings(), &mut errors);
    check_format_fits_framing(units, &imports.encodings(), &mut errors);
    validate_throws(units, &mut errors);
    check_duplicate_members(units, &mut errors);
    super::data::check_dataless_arrays(units, imports, &mut errors);

    if errors.is_empty() {
        Ok(warnings)
    } else {
        Err(errors)
    }
}

/// Names inside one declaration must differ (core#166): a struct's or an
/// error's fields, a validator's parameters, an enum's variants, a
/// protocol's functions, a function's arguments. A repeat makes the wire
/// ambiguous (which `a` is decoded?) and generates code that doesn't
/// compile. Each repeat is reported where it's written, pointing back at
/// the first.
fn check_duplicate_members(units: &[FrozenUnit], errors: &mut Vec<Diagnostic>) {
    fn repeats<'a>(
        members: impl IntoIterator<Item = (&'a str, (usize, usize))>,
    ) -> Vec<(&'a str, (usize, usize), (usize, usize))> {
        let mut first: HashMap<&str, (usize, usize)> = HashMap::new();
        let mut out = vec![];
        for (name, span) in members {
            // An unnamed argument (`function f(S)`) has no name to repeat.
            if name.is_empty() {
                continue;
            }
            match first.get(name) {
                Some(&at) => out.push((name, at, span)),
                None => {
                    first.insert(name, span);
                }
            }
        }
        out
    }
    let mut report = |code: DiagnosticCode, what: &str, owner: String, name: &str, first, again| {
        errors.push(
            Diagnostic::new(code, format!("{owner} has two {what}s named '{name}'"))
                .with_context(owner)
                .with_span(again)
                .with_secondary_span(first, "first declared here")
                .with_help(format!("rename one {what}, or remove the repeat")),
        );
    };
    fn fields(members: &[FrozenUnit]) -> Vec<(&str, (usize, usize))> {
        members
            .iter()
            .filter_map(|m| match m {
                FrozenUnit::Field { name, span, .. } => Some((name.as_str(), *span)),
                _ => None,
            })
            .collect()
    }
    for unit in units {
        match unit {
            FrozenUnit::Struct { name, fields: members, .. } => {
                for (field, first, again) in repeats(fields(members)) {
                    report(DUPLICATE_FIELD, "field", format!("struct '{name}'"), field, first, again);
                }
            }
            FrozenUnit::Error { name, fields: members, imported_from: None, .. } => {
                for (field, first, again) in repeats(fields(members)) {
                    report(DUPLICATE_FIELD, "field", format!("error '{name}'"), field, first, again);
                }
            }
            FrozenUnit::Validator { name, properties, .. } => {
                for (param, first, again) in repeats(fields(properties)) {
                    report(DUPLICATE_PARAMETER, "parameter", format!("validator '{name}'"), param, first, again);
                }
            }
            FrozenUnit::Enum { name, variants, .. } => {
                let named = variants.iter().filter_map(|v| match v {
                    FrozenUnit::EnumVariant(variant, span) => Some((variant.as_str(), *span)),
                    _ => None,
                });
                for (variant, first, again) in repeats(named) {
                    report(DUPLICATE_VARIANT, "variant", format!("enum '{name}'"), variant, first, again);
                }
            }
            FrozenUnit::Protocol { name, functions, .. } => {
                let named = functions.iter().filter_map(|f| match f {
                    FrozenUnit::Function { name, span, .. } => Some((name.as_str(), *span)),
                    _ => None,
                });
                for (function, first, again) in repeats(named) {
                    report(DUPLICATE_FUNCTION, "function", format!("protocol '{name}'"), function, first, again);
                }
                for function in functions {
                    let FrozenUnit::Function { name: function, arguments, .. } = function else {
                        continue;
                    };
                    let named = arguments.iter().map(|a| (a.name.as_str(), a.span));
                    for (argument, first, again) in repeats(named) {
                        report(
                            DUPLICATE_ARGUMENT,
                            "argument",
                            format!("function '{name}.{function}'"),
                            argument,
                            first,
                            again,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

/// Every validator names the type(s) it applies to (`for ...`), its
/// `value.*` accessors exist on those types, and every field it's applied
/// to has one of them. Matching is exact after resolving aliases (local,
/// or imported through `imports`): a `u8` field doesn't take a `for u32`
/// validator, and a validator on `string[]` checks the array, not each
/// element. `optional` is already outside the field's frozen type.
/// The `{path}` placeholders in a message template, `{{` / `}}` (literal
/// braces) skipped.
fn message_placeholders(message: &str) -> Vec<&str> {
    let mut paths = Vec::new();
    let mut rest = message;
    while let Some(open) = rest.find('{') {
        rest = &rest[open..];
        if let Some(after) = rest.strip_prefix("{{") {
            rest = after;
            continue;
        }
        let Some(close) = rest.find('}') else { break };
        paths.push(rest[1..close].trim());
        rest = &rest[close + 1..];
    }
    paths
}

/// `{path}` written in a plain (non-f) message prints as written: only an
/// f-string interpolates.
fn check_uninterpolated_messages(units: &[FrozenUnit], warnings: &mut Vec<Diagnostic>) {
    let warn = |paths: &[String], span: Option<(usize, usize)>, context: String| {
        let written = paths.iter().map(|p| format!("`{{{p}}}`")).collect::<Vec<_>>().join(", ");
        let mut d = Diagnostic::new(
            MESSAGE_NOT_INTERPOLATED,
            format!("{written} is printed as written: only an f-string interpolates"),
        )
        .with_context(context)
        .with_help("write the message as an f-string: `f\"...\"`");
        if let Some(span) = span {
            d = d.with_span(span);
        }
        d
    };
    for unit in units {
        match unit {
            FrozenUnit::Error { name, uninterpolated, message_span, .. } if !uninterpolated.is_empty() => {
                warnings.push(warn(uninterpolated, *message_span, format!("Error '{name}'")));
            }
            FrozenUnit::Validator { name, expression_block, .. } => {
                let FrozenUnit::ExpressionBlock { asserts } = &**expression_block else { continue };
                for assert in asserts {
                    if let FrozenUnit::Assert { uninterpolated, message_span, .. } = assert {
                        if !uninterpolated.is_empty() {
                            warnings.push(warn(uninterpolated, *message_span, format!("Validator '{name}'")));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

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
        for written in targets {
            if any_misplaced(written, false) {
                errors.push(
                    Diagnostic::new(
                        VALIDATOR_ANY_MISPLACED,
                        format!("validator '{name}': `any` only names an array's element, not `{written}`"),
                    )
                    .with_context(format!("Validator '{name}'"))
                    .with_span(*span)
                    .with_help("write `any[]` for an array of any element type"),
                );
            }
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

    for site in validated_sites(units) {
        let (field_written, span) = (site.ty, &site.span);
        let field_resolved = resolve(field_written);
        // An unknown type is already reported; matching a validator against
        // it would only add a second error about the same mistake.
        if names_unknown(&field_resolved, symbols, imports) {
            continue;
        }

        for param in site.parameters {
            let FrozenUnit::ValidatorRef { name: vname, span: call_span, .. } = param else {
                continue;
            };
            let span = &known_or(*call_span, *span);
            // A local validator without targets was reported above, and
            // an unknown name by pass 2.5 - neither has anything to match.
            let targets: Vec<(TypeRef, TypeRef)> = match local_targets.get(vname.as_str()) {
                Some(pairs) => pairs.clone(),
                None if could_be_imported(symbols, vname) => {
                    match imports.validator_targets(vname) {
                        Some(t) => t.into_iter().map(|w| (w.clone(), w)).collect(),
                        None => continue,
                    }
                }
                None => continue,
            };
            // Bounds don't take part (core#98): `for string` applies to
            // `char[max 32]`.
            let field_unbounded = field_resolved.without_bounds();
            if targets.is_empty() || targets.iter().any(|(_, r)| target_accepts(&r.without_bounds(), &field_unbounded)) {
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
                .with_context(site.context.clone())
                .with_span(*span),
            );
    }
    }
}

/// One place `@validators` may be written: a struct or error field, or a
/// `type` alias (whose checks then run wherever the alias is used). `ty` is
/// what the validators check - the field's type, or the alias's target.
struct ValidatedSite<'a> {
    context: String,
    parameters: &'a [FrozenUnit],
    ty: &'a TypeRef,
    span: (usize, usize),
}

fn validated_sites(units: &[FrozenUnit]) -> Vec<ValidatedSite<'_>> {
    let mut sites = Vec::new();
    for unit in units {
        match unit {
            FrozenUnit::Struct { name, fields, .. } | FrozenUnit::Error { name, fields, .. } => {
                let kind = if matches!(unit, FrozenUnit::Struct { .. }) { "Struct" } else { "Error" };
                for field in fields {
                    if let FrozenUnit::Field { name: field_name, parameters, ty, span, .. } = field {
                        sites.push(ValidatedSite {
                            context: format!("{kind} '{name}', field '{field_name}'"),
                            parameters,
                            ty,
                            span: *span,
                        });
                    }
                }
            }
            FrozenUnit::TypeAlias { name, parameters, target, span, .. } => {
                sites.push(ValidatedSite {
                    context: format!("Type alias '{name}'"),
                    parameters,
                    ty: target,
                    span: *span,
                });
            }
            _ => {}
        }
    }
    sites
}

/// `@validators` anywhere but a [`validated_sites`] entry checks nothing -
/// a struct, error, protocol or function has no single value to check - so
/// it's an error rather than a silent no-op.
fn check_misplaced_validators(units: &[FrozenUnit], errors: &mut Vec<Diagnostic>) {
    let mut report = |context: String, span: Option<(usize, usize)>| {
        let diagnostic = Diagnostic::new(
            MISPLACED_VALIDATORS,
            "`@validators` goes on a field or a `type` alias".to_string(),
        )
        .with_context(context);
        let diagnostic = match span {
            Some(span) => diagnostic.with_span(span),
            None => diagnostic,
        };
        errors.push(
            diagnostic.with_help(
                "move it onto the field to check, or onto a `type` alias used where the \
                 check should apply"
                    .to_string(),
            ),
        );
    };
    let has_refs = |ps: &[FrozenUnit]| ps.iter().any(|p| matches!(p, FrozenUnit::ValidatorRef { .. }));
    for unit in units {
        match unit {
            FrozenUnit::Struct { name, parameters, span, .. } if has_refs(parameters) => {
                report(format!("Struct '{name}'"), Some(*span))
            }
            FrozenUnit::Error { name, parameters, span, .. } if has_refs(parameters) => {
                report(format!("Error '{name}'"), *span)
            }
            FrozenUnit::Protocol { name, parameters, functions, span, .. } => {
                if has_refs(parameters) {
                    report(format!("Protocol '{name}'"), Some(*span));
                }
                for function in functions {
                    if let FrozenUnit::Function { name: f, parameters, span, .. } = function {
                        if has_refs(parameters) {
                            report(format!("Protocol '{name}', function '{f}'"), Some(*span));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Whether `ty` still names something neither declared here nor imported -
/// or imported only as a [stand-in](ImportLookup::is_stand_in) - after alias
/// resolution.
fn names_unknown(ty: &TypeRef, symbols: &SymbolTable, imports: &dyn ImportLookup) -> bool {
    match ty {
        TypeRef::Named(name) => {
            (symbols.get(name).is_none() && !could_be_imported(symbols, name))
                || imports.is_stand_in(name)
        }
        TypeRef::Array { element, .. } => names_unknown(element, symbols, imports),
        TypeRef::Union(members) => members.iter().any(|m| names_unknown(m, symbols, imports)),
        TypeRef::Primitive(_) | TypeRef::Unit => false,
    }
}

/// Whether one of this schema's `use`s could bring `name` into scope.
/// Imports are keyed by their full `a::b::Name` path, a `a::b::*` glob, or
/// a whole namespace (`a::b`, then written `a::b::Name`).
///
/// Only such a name is put to an [`ImportLookup`]. A name nothing imports is
/// unknown, whatever a lookup could find for it elsewhere in the project -
/// asking anyway is how the editor once checked a validator's targets while
/// also reporting the validator unknown.
pub(super) fn could_be_imported(symbols: &SymbolTable, name: &str) -> bool {
    matches!(symbols.get(name), Some(SymbolType::Import))
        || symbols.symbols.iter().any(|(k, v)| {
            v.kind == SymbolType::Import
                && (*k == name
                    || k.ends_with(&format!("::{name}"))
                    || k.ends_with("::*")
                    || name.starts_with(&format!("{k}::")))
        })
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
            None if could_be_imported(symbols, name) => imports.alias_target(name),
            None => None,
        })
    }
    go(ty, local_aliases, symbols, imports, 0)
}

/// The element name a validator's `for` uses for "any type": `any[]` is an
/// array of anything. A keyword only there; elsewhere `any` is a name.
const ANY: &str = "any";

/// Whether a validator target accepts a field's type, both resolved and
/// without bounds. `any` in an array's element accepts every element type;
/// otherwise the two must be equal.
fn target_accepts(target: &TypeRef, field: &TypeRef) -> bool {
    match (target, field) {
        (
            TypeRef::Array { element: t, length: tl, .. },
            TypeRef::Array { element: f, length: fl, .. },
        ) if tl == fl => matches!(&**t, TypeRef::Named(n) if n == ANY) || target_accepts(t, f),
        _ => target == field,
    }
}

/// Whether `any` appears in a target other than as an array's element.
fn any_misplaced(target: &TypeRef, in_array: bool) -> bool {
    match target {
        TypeRef::Named(n) => n == ANY && !in_array,
        TypeRef::Array { element, .. } => any_misplaced(element, true),
        TypeRef::Union(members) => members.iter().any(|m| any_misplaced(m, false)),
        _ => false,
    }
}

/// std's validators whose meaning is fixed, by the size they read: a
/// minimum (`MinChars`, `MinItems`, `StringBounds.min_chars`) and a cap
/// (`StringBounds.max_chars`). `(validator, property, for strings, is a
/// minimum)`.
const STD_SIZE_PROPERTIES: &[(&str, &str, bool, bool)] = &[
    ("MinChars", "min", true, true),
    ("MinItems", "min", false, true),
    ("StringBounds", "min_chars", true, true),
    ("StringBounds", "max_chars", true, false),
];

/// A std validator against the type it checks (core#98): a minimum above
/// the type's maximum refuses every value (an error); a cap below it
/// refuses values the type allows (a warning: the type is the place for
/// it). Only std's are judged, whose properties mean what their names say.
fn check_validator_sizes(
    units: &[FrozenUnit],
    symbols: &SymbolTable,
    imports: &dyn ImportLookup,
    constants: &Constants,
    errors: &mut Vec<Diagnostic>,
    warnings: &mut Vec<Diagnostic>,
) {
    let local_aliases: HashMap<&str, &TypeRef> = units
        .iter()
        .filter_map(|u| match u {
            FrozenUnit::TypeAlias { name, target, .. } => Some((name.as_str(), target)),
            _ => None,
        })
        .collect();
    let declared_here = |name: &str| {
        units.iter().any(|u| matches!(u, FrozenUnit::Validator { name: n, .. } if n == name))
    };
    for site in validated_sites(units) {
        let resolved = resolve_type(site.ty, &local_aliases, symbols, imports);
        let (is_string, size) = match &resolved {
            TypeRef::Array { element, length, max } => {
                (**element == TypeRef::Primitive(PrimitiveType::Char), max.or(*length))
            }
            _ => continue,
        };
        let Some(size) = size else { continue };
        for param in site.parameters {
            let FrozenUnit::ValidatorRef { name, args, span, .. } = param else { continue };
            let bare = name.rsplit("::").next().unwrap_or(name);
            if declared_here(bare) || !std_validator(symbols, name) {
                continue;
            }
            for (validator, property, for_strings, minimum) in STD_SIZE_PROPERTIES {
                if *validator != bare || *for_strings != is_string {
                    continue;
                }
                let value = args.iter().find_map(|arg| match arg {
                    FrozenUnit::Property { name, value } if name == property => integer_argument(value, constants),
                    _ => None,
                });
                let Some(value) = value else { continue };
                let unit = if is_string { "characters" } else { "items" };
                let at = known_or(*span, site.span);
                if *minimum && value > size as i128 {
                    errors.push(
                        Diagnostic::new(
                            VALIDATOR_MINIMUM_ABOVE_MAXIMUM,
                            format!(
                                "`{bare}({property} = {value})` refuses every value: `{}` holds at most {size} {unit}",
                                site.ty
                            ),
                        )
                        .with_context(site.context.clone())
                        .with_span(at),
                    );
                } else if !*minimum && value < size as i128 {
                    warnings.push(
                        Diagnostic::new(
                            VALIDATOR_CAPS_BELOW_TYPE,
                            format!(
                                "`{bare}({property} = {value})` refuses values `{}` allows (up to {size} {unit})",
                                site.ty
                            ),
                        )
                        .with_context(site.context.clone())
                        .with_span(at)
                        .with_help(format!("put the maximum in the type: `char[max {value}]`")),
                    );
                }
            }
        }
    }
}

/// Whether the validator `name` is std's: imported from `std::validators`.
fn std_validator(symbols: &SymbolTable, name: &str) -> bool {
    if name.starts_with("std::validators::") {
        return true;
    }
    let bare = name.rsplit("::").next().unwrap_or(name);
    symbols.symbols.iter().any(|(k, v)| {
        v.kind == SymbolType::Import
            && (*k == format!("std::validators::{bare}") || *k == "std::validators::*" || *k == "std::validators")
    })
}

/// An integer argument, written or through a constant.
fn integer_argument(value: &ConfigValue, constants: &Constants) -> Option<i128> {
    match value {
        ConfigValue::Integer(i) => Some(*i as i128),
        ConfigValue::Name(r) | ConfigValue::Path(r) => match constants.resolve(r) {
            Resolution::Value(ResolvedConstant { value: super::constants::ConstantValue::Integer(i), .. }) => Some(i),
            _ => None,
        },
        _ => None,
    }
}

/// Whether `value.length` means something for this (resolved) type.
///
/// What it means is fixed here, for every target to emit the same way: a
/// `string`'s length counts Unicode scalar values (Rust's
/// `.chars().count()`, not its UTF-8 byte length, and not JavaScript's
/// UTF-16 `.length`); an array's counts its elements.
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
fn check_annotation_values(units: &[FrozenUnit], imports: &dyn ImportLookup, errors: &mut Vec<Diagnostic>) {
    for unit in units {
        match unit {
            FrozenUnit::Struct { name, parameters, fields, span, .. } => {
                check_params(parameters, &format!("struct '{name}'"), Some(*span), imports, errors);
                check_field_params(fields, imports, errors);
            }
            FrozenUnit::Error { name, parameters, fields, span, .. } => {
                check_params(parameters, &format!("error '{name}'"), *span, imports, errors);
                check_field_params(fields, imports, errors);
            }
            FrozenUnit::Protocol { name, parameters, functions, span, .. } => {
                check_params(parameters, &format!("protocol '{name}'"), Some(*span), imports, errors);
                for function in functions {
                    if let FrozenUnit::Function { name: fname, parameters, span, .. } = function {
                        check_params(
                            parameters,
                            &format!("protocol '{name}', function '{fname}'"),
                            Some(*span),
                            imports,
                            errors,
                        );
                    }
                }
            }
            FrozenUnit::TypeAlias { name, parameters, span, .. } => {
                check_params(parameters, &format!("type alias '{name}'"), Some(*span), imports, errors);
            }
            _ => {}
        }
    }
}

/// `@unverified`: required on a protocol that gives up a guarantee, with a
/// reason; a warning where nothing is given up (like an unused `unsafe`);
/// and one informational warning per guarantee given up, each its own code
/// so an organisation can raise or quiet it (`diagnostics.<code>.level`).
fn check_unverified(
    units: &[FrozenUnit],
    encodings: &crate::schema::ir::unverified::Encodings,
    errors: &mut Vec<Diagnostic>,
    warnings: &mut Vec<Diagnostic>,
) {
    use crate::schema::ir::frozen::unit::Guarantee;
    use crate::schema::ir::unverified;

    for unit in units {
        let FrozenUnit::Protocol { name, parameters, span, .. } = unit else {
            continue;
        };
        let mut given_up = unverified::given_up(parameters, encodings);
        // Bounded size is worked out from the types a protocol reaches, by
        // the build's bounds pass, and frozen beside the rest (core#98).
        if unverified::frozen_set(parameters).contains(&Guarantee::BoundedSize) {
            given_up.push(Guarantee::BoundedSize);
        }
        let cause = unverified::cause(parameters, encodings);
        let Some(marker) = unverified::marker(parameters) else {
            if !given_up.is_empty() {
                errors.push(
                    Diagnostic::new(
                        UNVERIFIED_MISSING_MARKER,
                        format!(
                            "protocol `{name}` gives up {} ({cause}) without `@unverified`",
                            unverified::describe(&given_up)
                        ),
                    )
                    .with_help(
                        "say why with `@unverified = \"...\"` on the protocol, and allow the \
                         marker in settings (`annotations.unverified.allowed = true`)"
                            .to_string(),
                    )
                    .maybe_span(Some(*span)),
                );
            }
            continue;
        };
        if matches!(marker, ConfigValue::Str(reason) if reason.trim().is_empty()) {
            errors.push(
                Diagnostic::new(
                    UNVERIFIED_MISSING_REASON,
                    format!("`@unverified` on protocol `{name}` needs a reason"),
                )
                .with_help("every consumer is shown it: say what replaces the check".to_string())
                .maybe_span(Some(*span)),
            );
        }
        let why = |guarantee: Guarantee| match guarantee {
            Guarantee::BoundedSize => "a string, list or nesting it reaches has no maximum".to_string(),
            _ => unverified::cause_of(parameters, encodings, guarantee),
        };
        // The keyed form: a reason for every guarantee given up, and only
        // those, under a guarantee's id.
        if let ConfigValue::Dict(entries) = marker {
            let mut listed: Vec<Guarantee> = vec![];
            for (key, value) in entries {
                let Some(guarantee) = Guarantee::from_name(key) else {
                    let mut message = format!("`@unverified` on protocol `{name}`: `{key}` isn't a guarantee");
                    if let Some(s) = closest(key, Guarantee::ALL.iter().map(|g| g.name())) {
                        message.push_str(&format!(" - did you mean `{s}`?"));
                    }
                    let known: Vec<String> = Guarantee::ALL.iter().map(|g| format!("`{}`", g.name())).collect();
                    errors.push(
                        Diagnostic::new(UNVERIFIED_UNKNOWN_GUARANTEE, message)
                            .with_help(format!("the guarantees are {}", known.join(", ")))
                            .maybe_span(Some(*span)),
                    );
                    continue;
                };
                if listed.contains(&guarantee) {
                    errors.push(
                        Diagnostic::new(
                            UNVERIFIED_DUPLICATE_GUARANTEE,
                            format!("`@unverified` on protocol `{name}` gives a reason for `{key}` twice"),
                        )
                        .maybe_span(Some(*span)),
                    );
                    continue;
                }
                listed.push(guarantee);
                if !matches!(value, ConfigValue::Str(reason) if !reason.trim().is_empty()) {
                    errors.push(
                        Diagnostic::new(
                            UNVERIFIED_MISSING_REASON,
                            format!("`@unverified` on protocol `{name}` needs a reason for `{key}`, as a string"),
                        )
                        .with_help("every consumer is shown it: say what replaces the check".to_string())
                        .maybe_span(Some(*span)),
                    );
                }
            }
            for guarantee in given_up.iter().filter(|g| !listed.contains(g)) {
                errors.push(
                    Diagnostic::new(
                        UNVERIFIED_MISSING_GUARANTEE,
                        format!(
                            "protocol `{name}` gives up {} ({}) but `@unverified` doesn't say why",
                            guarantee.describe(),
                            why(*guarantee)
                        ),
                    )
                    .with_help(format!("add `{} = \"...\"` to `@unverified`", guarantee.name()))
                    .maybe_span(Some(*span)),
                );
            }
            for guarantee in listed.iter().filter(|g| !given_up.contains(g)) {
                warnings.push(
                    Diagnostic::new(
                        UNVERIFIED_NOT_GIVEN_UP,
                        format!(
                            "`@unverified` on protocol `{name}` lists `{}`, which it doesn't give up",
                            guarantee.name()
                        ),
                    )
                    .with_help(format!("remove `{}` from `@unverified`", guarantee.name()))
                    .maybe_span(Some(*span)),
                );
            }
        }
        if given_up.is_empty() {
            warnings.push(
                Diagnostic::new(
                    UNVERIFIED_NOTHING_GIVEN_UP,
                    format!("protocol `{name}` is marked `@unverified` but gives nothing up ({cause})"),
                )
                .with_help("remove the marker".to_string())
                .maybe_span(Some(*span)),
            );
        }
        for guarantee in given_up {
            let code = match guarantee {
                Guarantee::SchemaAgreement => UNVERIFIED_SCHEMA_AGREEMENT,
                Guarantee::EncodingAgreement => UNVERIFIED_ENCODING_AGREEMENT,
                Guarantee::EncodingConformance => UNVERIFIED_ENCODING_CONFORMANCE,
                Guarantee::VersionHonesty => UNVERIFIED_VERSION_HONESTY,
                Guarantee::BoundedSize => UNVERIFIED_BOUNDED_SIZE,
            };
            warnings.push(
                Diagnostic::new(
                    code,
                    format!(
                        "protocol `{name}` is unverified: {} isn't established ({})",
                        guarantee.describe(),
                        why(guarantee)
                    ),
                )
                .maybe_span(Some(*span)),
            );
        }
    }
}

/// `@provider` and `@consumer`: if both are written they have to be
/// counterparts, and calls to a protocol the client provides (or both
/// sides do) need a framing whose frames tell requests from replies.
/// `datagram` (`@1`) can't: a frame the server starts would be read as a
/// reply.
fn check_provider(units: &[FrozenUnit], encodings: &crate::schema::ir::unverified::Encodings, errors: &mut Vec<Diagnostic>) {
    for unit in units {
        let FrozenUnit::Protocol { name, parameters, span, .. } = unit else {
            continue;
        };
        let side = |key: &str| {
            parameters.iter().find_map(|p| match p {
                FrozenUnit::Property { name, value: ConfigValue::Name(side) } if name == key => {
                    annotations::SIDES.contains(&side.as_str()).then_some(side.as_str())
                }
                _ => None,
            })
        };
        let provider = side("provider");
        if let (Some(provider), Some(consumer)) = (provider, side("consumer")) {
            let counterpart = match provider {
                "Server" => "Client",
                "Client" => "Server",
                _ => "Multiple",
            };
            if consumer != counterpart {
                errors.push(
                    Diagnostic::new(
                        PROVIDER_CONSUMER_MISMATCH,
                        format!(
                            "protocol `{name}` is provided by `{provider}`, so its consumer is                              `{counterpart}`, not `{consumer}`"
                        ),
                    )
                    .with_help(format!(
                        "remove `@consumer`, which follows from `@provider`, or write                          `@consumer = {counterpart}`"
                    ))
                    .maybe_span(Some(*span)),
                );
            }
        }

        if !matches!(provider, Some("Client" | "Multiple")) {
            continue;
        }
        // A protocol's framing (written, the package's or core's default,
        // `datagram@3`) carries calls both ways unless it states otherwise.
        let framing = crate::schema::ir::unverified::framing(parameters, encodings);
        if crate::schema::ir::unverified::one_way_only(framing, encodings) {
            let framing = crate::encodings::official::profile(crate::encodings::EncodingKind::Framing, framing)
                .map_or(framing, |profile| profile.name.as_str());
            let provider = provider.unwrap_or_default();
            errors.push(
                Diagnostic::new(
                    REVERSE_CALLS_NEED_FRAMING,
                    format!(
                        "protocol `{name}` is provided by `{provider}`, so the server calls it, \
                         and the `{framing}` framing can't tell the server's requests from replies"
                    ),
                )
                .with_help(format!(
                    "use a framing that says which is which, such as `{}` (the default) or `\"jsonrpc\"`; \
                     if the package's `wire.framing` set `{framing}`, give this protocol its own `@framing`",
                    annotations::DEFAULT_FRAMING
                ))
                .maybe_span(Some(*span)),
            );
        }
    }
}

/// A protocol's framing has to be able to carry what its format writes
/// ([`unverified::misfit`]): caught here, as no runtime checks it on the wire.
fn check_format_fits_framing(units: &[FrozenUnit], encodings: &crate::schema::ir::unverified::Encodings, errors: &mut Vec<Diagnostic>) {
    for unit in units {
        let FrozenUnit::Protocol { name, parameters, span, .. } = unit else {
            continue;
        };
        if let Some((framing, grammar, format)) = crate::schema::ir::unverified::misfit(parameters, encodings) {
            errors.push(
                Diagnostic::new(
                    FORMAT_DOESNT_FIT_FRAMING,
                    format!(
                        "protocol `{name}` uses the framing `{framing}`, which carries {grammar}, but the \
                         package declares `wire.format = \"{format}\"`, which doesn't write that"
                    ),
                )
                .with_help(format!(
                    "declare a format that writes {grammar}, leave `wire.format` out for `{framing}`'s \
                     default, or give `{name}` another framing"
                ))
                .maybe_span(Some(*span)),
            );
        }
    }
}

fn check_field_params(fields: &[FrozenUnit], imports: &dyn ImportLookup, errors: &mut Vec<Diagnostic>) {
    for field in fields {
        if let FrozenUnit::Field { name, parameters, span, .. } = field {
            check_params(parameters, &format!("field '{name}'"), Some(*span), imports, errors);
        }
    }
}

fn check_params(
    parameters: &[FrozenUnit],
    context: &str,
    span: Option<(usize, usize)>,
    imports: &dyn ImportLookup,
    errors: &mut Vec<Diagnostic>,
) {
    use crate::encodings::{is_namespaced, undeclared, EncodingKind};

    for param in parameters {
        let FrozenUnit::Property { name, value } = param else {
            continue;
        };
        let Some(info) = annotations::lookup(name) else {
            continue;
        };
        // A handshake name gets the sentence `check_handshake` writes, and a
        // namespaced one has to be declared.
        if let (true, ConfigValue::Str(written)) = (name == "handshake", value) {
            let message = match annotations::check_handshake(written) {
                Ok(_) => None,
                Err(_) if imports.encoding_declared(EncodingKind::Handshake, written) => None,
                Err(_) if is_namespaced(written) => Some(undeclared(EncodingKind::Handshake, written)),
                Err(message) => Some(message),
            };
            if let Some(message) = message {
                errors.push(
                    Diagnostic::new(ANNOTATION_VALUE_MISMATCH, message)
                        .with_context(context.to_string())
                        .maybe_span(span),
                );
            }
            continue;
        }
        // The dictionary form: its name as above, and the parameters that
        // handshake takes, the same check as `config.idp`'s.
        if let (true, ConfigValue::Dict(entries)) = (name == "handshake", value) {
            let message = match value.entry("name") {
                Some(ConfigValue::Str(written)) => {
                    let known = annotations::check_handshake(written).is_ok();
                    if !known && is_namespaced(written) && !imports.encoding_declared(EncodingKind::Handshake, written) {
                        Some(undeclared(EncodingKind::Handshake, written))
                    } else if !known && !is_namespaced(written) {
                        annotations::check_handshake(written).err()
                    } else {
                        let canonical = annotations::canonical_value("handshake", written).unwrap_or(written);
                        let mut parameters = vec![];
                        let mut malformed = None;
                        for (key, parameter) in entries.iter().filter(|(k, _)| k != "name") {
                            match parameter {
                                ConfigValue::Str(v) | ConfigValue::Name(v) => parameters.push((key.as_str(), v.as_str())),
                                _ => malformed = Some(format!("'@handshake': `{key}` should be a string or a bare word")),
                            }
                        }
                        malformed.or_else(|| annotations::check_handshake_params(canonical, parameters).err())
                    }
                }
                _ => Some("'@handshake': the dictionary form needs a `name`, like { name = \"comline@2\", scope = package }".to_string()),
            };
            if let Some(message) = message {
                errors.push(
                    Diagnostic::new(ANNOTATION_VALUE_MISMATCH, message)
                        .with_context(context.to_string())
                        .maybe_span(span),
                );
            }
            continue;
        }
        // A namespaced framing, `"com.acme.x@1"` or `{ name = "com.acme.x@1" }`:
        // fine when declared, and otherwise an error saying so, never a
        // silent default (cli#81).
        if name == "framing" {
            let written = match value {
                ConfigValue::Str(written) => Some(written),
                ConfigValue::Dict(_) => match value.entry("name") {
                    Some(ConfigValue::Str(written)) => Some(written),
                    _ => None,
                },
                _ => None,
            };
            if let Some(written) = written.filter(|w| is_namespaced(w)) {
                let mut problem = None;
                if !imports.encoding_declared(EncodingKind::Framing, written) {
                    problem = Some(undeclared(EncodingKind::Framing, written));
                } else if let ConfigValue::Dict(entries) = value {
                    if let Some((key, _)) = entries.iter().find(|(k, _)| k != "name" && k != "method_names") {
                        problem = Some(format!("'@framing': `{key}` isn't allowed: expected `name` or `method_names`"));
                    } else if value.entry("method_names").is_some() {
                        if !crate::schema::ir::unverified::names_calls(written, &imports.encodings()) {
                            problem = Some(names_no_calls(written));
                        } else if annotations::MethodNames::of(value).is_none() {
                            problem = Some("'method_names' must be `bare` or `qualified`".to_string());
                        }
                    }
                }
                if let Some(message) = problem {
                    errors.push(
                        Diagnostic::new(ANNOTATION_VALUE_MISMATCH, message)
                            .with_context(context.to_string())
                            .maybe_span(span),
                    );
                }
                continue;
            }
        }
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
            continue;
        }
        // `@unverified`'s keys are guarantees, which `check_unverified` reports
        // with codes of its own.
        if let (ConfigValue::Dict(entries), Some(keys), false) = (value, config_value::dict_keys(&info.value), name == "unverified") {
            let checked = config_value::check_entries(keys, entries.iter().map(|(k, v)| (k.as_str(), v.form())));
            if let Err(problems) = checked {
                for problem in problems {
                    errors.push(
                        Diagnostic::new(ANNOTATION_VALUE_MISMATCH, format!("'@{name}': {problem}"))
                            .with_context(context.to_string())
                            .with_help(format!("'@{name}' takes {}", config_value::describe(&info.value)))
                            .maybe_span(span),
                    );
                }
                continue;
            }
        }
        // `method_names` says how a framing names a call; one that doesn't
        // has nothing it could change.
        if name == "framing" && value.entry("method_names").is_some() {
            if let Some(framing) = annotations::FramingValue::of(value)
                .filter(|f| !crate::schema::ir::unverified::names_calls(f.name, &imports.encodings()))
            {
                errors.push(
                    Diagnostic::new(ANNOTATION_VALUE_MISMATCH, names_no_calls(framing.name))
                        .with_context(context.to_string())
                        .with_help("remove `method_names`, or use a framing that does, such as `\"jsonrpc\"`".to_string())
                        .maybe_span(span),
                );
            }
        }
    }
}

/// The sentence for `method_names` on a framing that doesn't name its calls.
fn names_no_calls(framing: &str) -> String {
    format!("'@framing': `method_names` applies to a framing that names its calls (`calls = by_name`), not `{framing}`")
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

/// Whether an `optional` field ends a cycle. A struct holding an optional
/// copy of itself (a linked list) is finite, but generated Rust would need
/// the field boxed (`Option<Box<S>>`) to have a size, and the Rust
/// generator doesn't box yet. Until it does, `optional` doesn't end a cycle
/// (core#167).
const OPTIONAL_ENDS_A_CYCLE: bool = false;

/// Structs that can never hold a finite value: every value of them contains
/// another value of them (core#167). Found as a least fixpoint, so every
/// way a value can end counts: a dynamic array (it can be empty), a fixed
/// array of length 0, a union with a member that can end, a type alias
/// whose target can, and every type that isn't a struct. Aliases are
/// resolved through the schema's own `type` declarations; a name declared
/// elsewhere is taken as able to end, since its own schema checks it.
///
/// Only a struct whose value leads back to itself is reported, under
/// `validation::struct-cycle`; one that merely contains such a struct
/// isn't blamed for it.
fn check_struct_cycles(units: &[FrozenUnit], errors: &mut Vec<Diagnostic>) {
    let structs: Vec<(&str, &[FrozenUnit], (usize, usize))> = units
        .iter()
        .filter_map(|u| match u {
            FrozenUnit::Struct { name, fields, span, .. } => Some((name.as_str(), fields.as_slice(), *span)),
            _ => None,
        })
        .collect();
    let aliases: HashMap<&str, &TypeRef> = units
        .iter()
        .filter_map(|u| match u {
            FrozenUnit::TypeAlias { name, target, .. } => Some((name.as_str(), target)),
            _ => None,
        })
        .collect();
    let is_struct: HashSet<&str> = structs.iter().map(|(n, _, _)| *n).collect();

    // `seen` guards against an alias cycle, which alias resolution reports.
    fn ends<'a>(
        ty: &'a TypeRef,
        finite: &HashSet<&str>,
        is_struct: &HashSet<&str>,
        aliases: &HashMap<&str, &'a TypeRef>,
        seen: &mut Vec<&'a str>,
    ) -> bool {
        match ty {
            TypeRef::Primitive(_) | TypeRef::Unit => true,
            TypeRef::Array { length: None, .. } | TypeRef::Array { length: Some(0), .. } => true,
            TypeRef::Array { element, .. } => ends(element, finite, is_struct, aliases, seen),
            TypeRef::Union(members) => members.iter().any(|m| ends(m, finite, is_struct, aliases, seen)),
            TypeRef::Named(name) => {
                let name = name.as_str();
                if let Some(target) = aliases.get(name) {
                    if seen.contains(&name) {
                        return true;
                    }
                    seen.push(name);
                    let ends_ = ends(target, finite, is_struct, aliases, seen);
                    seen.pop();
                    ends_
                } else if is_struct.contains(name) {
                    finite.contains(name)
                } else {
                    true
                }
            }
        }
    }
    let field_ends = |field: &FrozenUnit, finite: &HashSet<&str>| match field {
        FrozenUnit::Field { optional, ty, .. } => {
            (*optional && OPTIONAL_ENDS_A_CYCLE) || ends(ty, finite, &is_struct, &aliases, &mut vec![])
        }
        _ => true,
    };

    let mut finite: HashSet<&str> = HashSet::new();
    loop {
        let before = finite.len();
        for (name, fields, _) in &structs {
            if !finite.contains(name) && fields.iter().all(|f| field_ends(f, &finite)) {
                finite.insert(name);
            }
        }
        if finite.len() == before {
            break;
        }
    }

    // The structs each infinite struct's never-ending fields reach.
    fn named_in<'a>(ty: &'a TypeRef, aliases: &HashMap<&str, &'a TypeRef>, out: &mut Vec<&'a str>, depth: usize) {
        if depth > 64 {
            return;
        }
        match ty {
            TypeRef::Named(name) => match aliases.get(name.as_str()) {
                Some(target) => named_in(target, aliases, out, depth + 1),
                None => out.push(name.as_str()),
            },
            TypeRef::Array { element, .. } => named_in(element, aliases, out, depth + 1),
            TypeRef::Union(members) => members.iter().for_each(|m| named_in(m, aliases, out, depth + 1)),
            _ => {}
        }
    }
    let reaches: HashMap<&str, Vec<&str>> = structs
        .iter()
        .filter(|(name, _, _)| !finite.contains(name))
        .map(|(name, fields, _)| {
            let mut out = vec![];
            for field in fields.iter().filter(|f| !field_ends(f, &finite)) {
                if let FrozenUnit::Field { ty, .. } = field {
                    named_in(ty, &aliases, &mut out, 0);
                }
            }
            out.retain(|n| is_struct.contains(n) && !finite.contains(n));
            (*name, out)
        })
        .collect();

    for (name, _, span) in &structs {
        if finite.contains(name) {
            continue;
        }
        // Does `name` reach itself through never-ending fields?
        let mut stack: Vec<&str> = reaches.get(name).cloned().unwrap_or_default();
        let mut visited: HashSet<&str> = HashSet::new();
        let mut on_cycle = false;
        while let Some(next) = stack.pop() {
            if next == *name {
                on_cycle = true;
                break;
            }
            if visited.insert(next) {
                stack.extend(reaches.get(next).into_iter().flatten().copied());
            }
        }
        if on_cycle {
            errors.push(
                Diagnostic::new(STRUCT_CYCLE, format!("Cycle detected involving struct '{name}'"))
                    .with_context(format!(
                        "every value of '{name}' contains another '{name}', so none can be written"
                    ))
                    .with_span(*span)
                    .with_help(
                        "end the cycle with a dynamic array (`T[]`), which can be empty, or a \
                         union member that doesn't lead back",
                    ),
            );
        }
    }
}

/// A literal written for a value of type `primitive` - a field or property
/// default, a constant, a validator argument - has to be that type, and an
/// integer has to fit the width declared. A reference (`MAX`, `u32::MAX`)
/// is judged by the value it stands for (core#168).
fn check_literal(
    primitive: PrimitiveType,
    literal: &Literal,
    constants: &Constants,
    context: &str,
    span: (usize, usize),
    errors: &mut Vec<Diagnostic>,
) {
    let fits = match (primitive, literal) {
        (p, Literal::Reference(reference)) => {
            return check_resolved(p, reference, constants.resolve(reference), context, span, errors)
        }
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
        // A `char` is written as a string of exactly one character.
        (PrimitiveType::Char, Literal::String(text)) => {
            let count = text.chars().count();
            if count != 1 {
                errors.push(
                    Diagnostic::new(
                        VALUE_OUT_OF_RANGE,
                        format!("a `char` is one character, found {count} in {literal}"),
                    )
                    .with_context(context.to_string())
                    .with_span(span),
                );
            }
            return;
        }
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

/// A string default against its type's length or maximum, in characters:
/// `char[max 3] = "abcd"` doesn't fit.
fn check_text_size(
    default: &Literal,
    length: Option<u64>,
    max: Option<u64>,
    constants: &Constants,
    context: &str,
    span: (usize, usize),
    errors: &mut Vec<Diagnostic>,
) {
    let text = match default {
        Literal::String(text) => text.clone(),
        Literal::Reference(r) => match constants.resolve(r) {
            Resolution::Value(ResolvedConstant { value: super::constants::ConstantValue::String(text), .. }) => text,
            _ => return,
        },
        _ => return,
    };
    let count = text.chars().count() as u64;
    let problem = match (length, max) {
        (Some(n), _) if count != n => Some(format!("exactly {n}")),
        (_, Some(n)) if count > n => Some(format!("at most {n}")),
        _ => None,
    };
    if let Some(holds) = problem {
        errors.push(
            Diagnostic::new(
                VALUE_OUT_OF_RANGE,
                format!(
                    "the default {text:?} is {count} character{}; the type holds {holds}",
                    if count == 1 { "" } else { "s" }
                ),
            )
            .with_context(context.to_string())
            .with_span(span),
        );
    }
}

/// [`check_literal`] for a `reference`, once resolved: the name has to be a
/// constant (or an integer type's bound), and its value has to fit
/// `primitive` as a literal would.
fn check_resolved(
    primitive: PrimitiveType,
    reference: &str,
    resolution: Resolution,
    context: &str,
    span: (usize, usize),
    errors: &mut Vec<Diagnostic>,
) {
    let diagnostic = match resolution {
        Resolution::Unjudged => return,
        Resolution::Value(resolved) if !super::constants::kind_fits(primitive, resolved.ty) => {
            Diagnostic::new(
                LITERAL_TYPE_MISMATCH,
                format!("expected a `{primitive}` value, found `{reference}`, which is a `{}`", resolved.ty),
            )
        }
        Resolution::Value(resolved) if !super::constants::in_range(primitive, &resolved.value) => {
            Diagnostic::new(
                VALUE_OUT_OF_RANGE,
                format!("`{reference}` is {}, which does not fit in `{primitive}`", resolved.value),
            )
            .with_help(format!(
                "name a constant `{primitive}` can hold, or widen the declared type"
            ))
        }
        Resolution::Value(_) => return,
        Resolution::Problem(Problem::Unknown { suggestion }) => {
            let mut message = format!("no constant named `{reference}`");
            if let Some(s) = suggestion {
                message.push_str(&format!(" - did you mean `{s}`?"));
            }
            Diagnostic::new(UNKNOWN_CONSTANT, message).with_help(format!(
                "declare it with `const {reference}: {primitive} = ...`, or import it with `use`"
            ))
        }
        Resolution::Problem(Problem::NotAConstant(kind)) => Diagnostic::new(
            NOT_A_CONSTANT,
            format!("`{reference}` is {}, not a constant", kind.describe()),
        ),
        Resolution::Problem(Problem::NoBound { ty, bound }) => {
            let help = if ty.is_integer() {
                format!("an integer type has `{ty}::MIN` and `{ty}::MAX`")
            } else {
                "only an integer type has `MIN` and `MAX`".to_string()
            };
            Diagnostic::new(UNKNOWN_CONSTANT, format!("`{ty}` has no `{bound}`")).with_help(help)
        }
        Resolution::Problem(Problem::Cycle(chain)) => Diagnostic::new(
            CONSTANT_CYCLE,
            format!("constant '{}' refers back to itself: {}", chain[0], chain.join(" -> ")),
        )
        .with_help("give one of them a literal value"),
    };
    errors.push(diagnostic.with_context(context.to_string()).with_span(span));
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
                errors.push(Diagnostic { code: UNKNOWN_TYPE, help: hint, secondary: None, severity_override: None, fix: None,
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
            // Members that differ only by bound are one shape on the wire:
            // nothing in `"abc"` says which of `char[max 3]` and
            // `char[max 5]` it was sent as (core#98).
            for (i, a) in members.iter().enumerate() {
                if let Some(b) = members[..i].iter().find(|b| *b != a && b.without_bounds() == a.without_bounds()) {
                    errors.push(
                        Diagnostic::new(
                            UNION_MEMBERS_DIFFER_ONLY_BY_BOUND,
                            format!("union members `{b}` and `{a}` differ only by bound"),
                        )
                        .with_context(context.to_string())
                        .with_help("a decoder can't tell them apart: keep one, with the larger bound")
                        .with_span(span),
                    );
                }
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
/// The keyword arguments of one `@validators` call against the validator's
/// properties (`fields`): each names a property, at most once, with a value of
/// its type, and every property without a default is given.
///
/// An argument's error points at its name (`arg_spans`, by index into
/// `args`), anything about the call as a whole at `span`.
#[allow(clippy::too_many_arguments)]
fn check_validator_arguments(
    vname: &str,
    fields: &[FrozenUnit],
    args: &[FrozenUnit],
    arg_spans: &[(usize, usize)],
    constants: &Constants,
    context: &str,
    span: (usize, usize),
    errors: &mut Vec<Diagnostic>,
) {
    let props = prop_names(fields);
    let mut error = |code, message, span| {
        errors.push(Diagnostic { code, help: None, secondary: None, severity_override: None, fix: None,
            message,
            context: context.to_string(),
            span: Some(span),
        })
    };

    // What the arguments' values come to, reported after the loop: `error`
    // holds `errors` until then.
    let mut values = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    // Properties an unknown argument was taken for (`mni` - did you mean
    // `min`?): already reported, so not again as missing.
    let mut suggested: Vec<String> = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        let FrozenUnit::Property { name: kw, value: val } = arg else {
            continue;
        };
        let at = known_or(arg_spans.get(i).copied().unwrap_or_default(), span);
        if seen.contains(&kw.as_str()) {
            error(DUPLICATE_VALIDATOR_ARGUMENT, format!("duplicate argument '{}' to validator '{}'", kw, vname), at);
            continue;
        }
        seen.push(kw.as_str());
        if !props.contains(&kw.as_str()) {
            let mut message = format!("validator '{}' has no argument '{}'", vname, kw);
            if let Some(s) = closest(kw, props.iter().copied()) {
                message.push_str(&format!(" - did you mean '{}'?", s));
                suggested.push(s);
            }
            error(UNKNOWN_VALIDATOR_ARGUMENT, message, at);
            continue;
        }

        // The argument's value against the property's declared type. Both
        // sides are typed now: the value keeps the form it was written in,
        // and the declared type maps onto the same vocabulary - so this no
        // longer re-parses rendered text to guess a category.
        let want = prop_field(fields, kw).and_then(|f| match f {
            FrozenUnit::Field { ty, .. } => expected_value_type(ty),
            _ => None,
        });
        if let Some(mismatch) = want.and_then(|want| argument_mismatch(&want, val)) {
            error(
                VALIDATOR_ARGUMENT_TYPE_MISMATCH,
                format!(
                    "validator '{}': argument '{}' expects {}, got {}",
                    vname, kw, mismatch.expected, mismatch.found
                ),
                at,
            );
            continue;
        }
        // A constant's name is read for its value, and an integer has to fit
        // the property's width, as a default does.
        let primitive = prop_field(fields, kw).and_then(|f| match f {
            FrozenUnit::Field { ty, .. } => ty.as_primitive(),
            _ => None,
        });
        let value = match val {
            ConfigValue::Name(r) | ConfigValue::Path(r) => Some(Literal::Reference(r.clone())),
            ConfigValue::Integer(i) => Some(Literal::Integer(*i)),
            _ => None,
        };
        if let (Some(primitive), Some(value)) = (primitive, value) {
            let context = format!("{context}, validator '{vname}', argument '{kw}'");
            check_literal(primitive, &value, constants, &context, at, &mut values);
        }
    }

    // Every property without a default has to be given - a bare `Name` gives
    // none.
    for field in fields {
        if let FrozenUnit::Field { name: prop, default: None, .. } = field {
            if !seen.contains(&prop.as_str()) && !suggested.contains(prop) {
                error(MISSING_VALIDATOR_ARGUMENT, format!("validator '{}' is missing argument '{}'", vname, prop), span);
            }
        }
    }
    errors.extend(values);
}

/// `span`, unless it's the `(0, 0)` of a unit frozen without one.
fn known_or(span: (usize, usize), fallback: (usize, usize)) -> (usize, usize) {
    if span == (0, 0) { fallback } else { span }
}

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
        "any" => Some("`any` only names an array's element in a validator's `for` (`any[]`); a value needs a type".to_string()),
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
