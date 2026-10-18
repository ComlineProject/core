// Standard Uses
use std::collections::HashMap;

// Crate Uses
use crate::package::config::ir::context::ProjectContext;
use crate::schema::idl::grammar::{
    self, Annotation, AnnotationValue, Declaration, Expression, UsePath,
};
use crate::schema::ir::compiler::alias_resolution::check_aliases;
use crate::schema::ir::compiler::settings::resolution::check_settings_conflicts;
use crate::schema::ir::compiler::import_resolver::{
    check_imports, declared_symbol_names, find_schema_bringing_into_scope, resolve_use_to_schema,
    importable_names, ImportResolver,
};
use crate::schema::ir::frozen::types::{Condition, Literal, TypeRef};
use crate::schema::ir::compiler::Compile;
use crate::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};
use crate::diagnostics::Diagnostic;

// External Uses


pub struct IncrementalInterpreter {}

impl IncrementalInterpreter {
    /// Compile declarations with awareness of the rest of the project, so that
    /// `use`/`import` declarations pointing at other schemas in the same
    /// package resolve to real, verified references instead of raw path
    /// strings.
    pub fn from_declarations_with_context(
        declarations: Vec<rust_sitter::Spanned<Declaration>>,
        current_namespace: &[String],
        project_context: &ProjectContext,
    ) -> Vec<FrozenUnit> {
        Self::compile_declarations(declarations, Some((current_namespace, project_context)))
    }

    /// Like `compile_declarations`, but checks `use` statements (with
    /// `use_context`: each must resolve, see `check_imports`) and `type`
    /// aliases (duplicate names, cycles, unresolvable targets) first, and
    /// reports those as a dedicated `Result` instead of silently falling
    /// back to a per-use-site "Unknown type" diagnostic. The real `cli
    /// check`/`cli build` path (`interpret_context`) always does these checks
    /// itself before calling `from_declarations_with_context`; this is for
    /// callers (tests, or any future caller) that want the same guarantee
    /// without building a full `ProjectContext`-driven project.
    pub fn check_and_from_declarations(
        declarations: Vec<rust_sitter::Spanned<Declaration>>,
        use_context: Option<(&[String], &ProjectContext)>,
    ) -> Result<Vec<FrozenUnit>, Vec<Diagnostic>> {
        if let Some((current_namespace, project_context)) = use_context {
            check_imports(&declarations, current_namespace, project_context)?;
        }
        check_aliases(&declarations, use_context)?;
        check_settings_conflicts(&declarations)?;
        Ok(Self::compile_declarations(declarations, use_context))
    }
}

impl Compile for IncrementalInterpreter {
    type Output = Vec<FrozenUnit>;

    fn from_declarations(declarations: Vec<rust_sitter::Spanned<Declaration>>) -> Self::Output {
        Self::compile_declarations(declarations, None)
    }
}

impl IncrementalInterpreter {
    fn compile_declarations(
        declarations: Vec<rust_sitter::Spanned<Declaration>>,
        use_context: Option<(&[String], &ProjectContext)>,
    ) -> Vec<FrozenUnit> {
        tracing::debug!("Processing {} declarations...", declarations.len());

        // Resolve the schema's error space up front: every locally-declared
        // `error` gets an ordinal in declaration order, then every distinct
        // `! Name` a function throws that *isn't* local gets the next slot as
        // a re-exported import (resolved through the in-scope `use`s, or a
        // `<unresolved: Name>` marker slot). `throws` then freezes as `u16`s.
        let ErrorPlan {
            ordinals: error_ordinals,
            reexports,
        } = plan_error_space(&declarations, use_context);

        let mut frozen_units: Vec<FrozenUnit> = vec![];

        for spanned_decl in declarations {
            let span = spanned_decl.span;

            match spanned_decl.value {
                Declaration::Import(import) => {
                    // Legacy import support
                    frozen_units.push(FrozenUnit::Import(import.path(), None, span));
                }
                Declaration::Use(use_stmt) => {
                    let alias = use_stmt.alias.as_ref().map(|a| a.name.text.clone());
                    let units = match use_context {
                        Some((current_namespace, project_context)) => resolve_use_declaration(
                            project_context,
                            current_namespace,
                            &use_stmt.path,
                            alias,
                            span,
                        ),
                        None => vec![FrozenUnit::Import(
                            extract_use_path(&use_stmt.path),
                            alias,
                            span,
                        )],
                    };
                    frozen_units.extend(units);
                }
                Declaration::Const(const_decl) => {
                    let name = const_decl.name();
                    let type_def = const_decl.type_def();
                    let value = const_decl.value();

                    frozen_units.push(FrozenUnit::Constant {
                        docstring: const_decl.docstring(),
                        name,
                        ty: TypeRef::from_grammar(type_def),
                        value: Literal::from_grammar(value),
                        span,
                    });
                }
                Declaration::Struct(struct_def) => {
                    let struct_name = struct_def.name();
                    let fields = struct_def.fields();

                    let field_units: Vec<FrozenUnit> = fields
                        .iter()
                        .map(|field| {
                            let fname = field.name();
                            let field_type = field.field_type();

                            FrozenUnit::Field {
                                docstring: field.docstring(),
                                parameters: annotation_units(&field.annotations()),
                                optional: field.optional(),
                                name: fname,
                                ty: TypeRef::from_grammar(field_type),
                                default: field.default_value().and_then(Literal::from_grammar),
                                span: field.span,
                            }
                        })
                        .collect();

                    frozen_units.push(FrozenUnit::Struct {
                        docstring: struct_def.docstring(),
                        parameters: annotation_units(&struct_def.annotations()),
                        name: struct_name,
                        fields: field_units,
                        span,
                    });
                }
                Declaration::Enum(enum_def) => {
                    let enum_name = enum_def.name();
                    let variants = enum_def.variants();

                    let variant_units: Vec<FrozenUnit> = variants
                        .iter()
                        .map(|variant| {
                            FrozenUnit::EnumVariant(
                                variant.identifier().to_string(),
                                variant.span,
                            )
                        })
                        .collect();

                    frozen_units.push(FrozenUnit::Enum {
                        docstring: enum_def.docstring(),
                        name: enum_name,
                        variants: variant_units,
                        span,
                    });
                }
                Declaration::Protocol(protocol) => {
                    let protocol_name = protocol.name();
                    let functions = protocol.functions();

                    let function_units: Vec<FrozenUnit> = functions
                        .iter()
                        .map(|func| {
                            let func_name = func.name();
                            let args_opt = func.args();
                            let ret_opt = func.return_type();

                            let arguments = if let Some(arg_list) = args_opt {
                                let first_arg = arg_list.first();
                                let rest_args = arg_list.rest();

                                let mut args =
                                    vec![crate::schema::ir::frozen::unit::FrozenArgument {
                                        name: first_arg
                                            .name()
                                            .map(|n| n.as_str().to_string())
                                            .unwrap_or_else(|| "arg0".to_string()),
                                        ty: TypeRef::from_grammar(first_arg.arg_type()),
                                        span: first_arg.arg_type_span(),
                                    }];

                                for (i, comma_arg) in rest_args.iter().enumerate() {
                                    let arg = comma_arg.arg_type();
                                    args.push(crate::schema::ir::frozen::unit::FrozenArgument {
                                        name: arg
                                            .name()
                                            .map(|n| n.as_str().to_string())
                                            .unwrap_or_else(|| format!("arg{}", i + 1)),
                                        ty: TypeRef::from_grammar(arg.arg_type()),
                                        span: arg.arg_type_span(),
                                    });
                                }
                                args
                            } else {
                                vec![]
                            };

                            let return_type = ret_opt
                                .as_ref()
                                .map(|rt| TypeRef::from_grammar(rt.return_type()));

                            FrozenUnit::Function {
                                name: func_name,
                                parameters: annotation_units(&func.annotations()),
                                arguments,
                                _return: return_type,
                                docstring: func.docstring().unwrap_or_default(),
                                throws: func
                                    .throws()
                                    .into_iter()
                                    .map(|name| {
                                        // Every throw name was given a slot by
                                        // `plan_error_space`; the default is a
                                        // defensive fallback only.
                                        error_ordinals.get(&name).copied().unwrap_or(0)
                                    })
                                    .collect(),
                                span: func.span,
                            }
                        })
                        .collect();

                    frozen_units.push(FrozenUnit::Protocol {
                        docstring: protocol.docstring().unwrap_or_default(),
                        name: protocol_name,
                        functions: function_units,
                        parameters: annotation_units(&protocol.annotations()),
                        span,
                    });
                }
                Declaration::Error(error_decl) => {
                    // Local errors keep the ordinal `plan_error_space` gave
                    // them; `None` only if a Protocol arm somehow references a
                    // name before it's planned, which can't happen.
                    let ordinal = error_ordinals.get(&error_decl.name()).copied().unwrap_or(0);
                    frozen_units.push(frozen_error(&error_decl, ordinal, None, Some(span)));
                }
                Declaration::Settings(settings_def) => {
                    let entries: Vec<_> = settings_def
                        .entries()
                        .iter()
                        .map(|entry| entry.value.to_path_value())
                        .collect();

                    // `check_settings_conflicts` (run earlier by
                    // `check_and_from_declarations`) already rejects a
                    // conflicting block on the real `comline build`/`check`
                    // path - this lenient fallback only matters for a
                    // caller that skips that pre-pass entirely (e.g.
                    // `from_declarations`/`from_source`).
                    let values = crate::settings::desugar::desugar(&entries)
                        .unwrap_or_else(|_conflicts| crate::settings::desugar::desugar_lenient(&entries));

                    // Origin span per leaf, keyed the same dotted-path way
                    // `values` itself is read back via `get_path` - a
                    // diagnostics-only sidecar, not part of `desugar`'s
                    // nested tree (see `FrozenUnit::Settings`'s doc comment).
                    let spans = crate::settings::SettingsSpans(
                        settings_def
                            .entries()
                            .iter()
                            .map(|entry| (entry.value.key_path().join("."), entry.span))
                            .collect(),
                    );

                    frozen_units.push(FrozenUnit::Settings {
                        docstring: settings_def.docstring(),
                        name: settings_def.name(),
                        values,
                        spans,
                    });
                }
                Declaration::Validator(validator_def) => {
                    let properties = freeze_validator_properties(&validator_def);

                    let asserts = validator_def
                        .asserts()
                        .into_iter()
                        .map(|a| FrozenUnit::Assert {
                            condition: a.condition.reconstruct(),
                            message: a.message.reconstruct(),
                            references: a.condition.member_paths(),
                            expr: Condition::from_grammar(&a.condition),
                        })
                        .collect();

                    let targets =
                        validator_def.targets().into_iter().map(TypeRef::from_grammar).collect();

                    frozen_units.push(FrozenUnit::Validator {
                        docstring: validator_def.docstring(),
                        properties,
                        name: validator_def.name(),
                        targets,
                        expression_block: Box::new(FrozenUnit::ExpressionBlock { asserts }),
                        span,
                    });
                }
                Declaration::TypeAlias(alias) => {
                    // Transparent like Rust's own `type`: a reference to
                    // this alias elsewhere (a field, an argument, ...)
                    // freezes to a plain `TypeRef::Named` just like any
                    // struct/enum reference does - no substitution happens
                    // here. The alias's *own* target freezes the same way,
                    // one level, as written (an alias-of-alias stays a
                    // `Named` reference to the other alias, not flattened) -
                    // a generator renders it with the same type mapping it
                    // uses everywhere else, plus one new top-level case to
                    // emit the declaration itself.
                    frozen_units.push(FrozenUnit::TypeAlias {
                        docstring: alias.docstring(),
                        parameters: annotation_units(&alias.annotations()),
                        name: alias.name(),
                        target: TypeRef::from_grammar(alias.target_type()),
                        span,
                    });
                }
            }
        }

        // Re-exported foreign errors named by a `throws`, in ordinal order,
        // after the local declarations.
        frozen_units.extend(reexports);

        tracing::debug!("Generated {} IR units", frozen_units.len());
        for unit in &frozen_units {
            tracing::trace!("  {:?}", unit);
        }
        // Return the generated IR units for testing/validation
        frozen_units
    }
}

/// The resolved error space of one schema: every error name this schema can
/// throw, mapped to its schema-global ordinal, plus the `FrozenUnit::Error`
/// re-export slots for the foreign ones (in ordinal order).
struct ErrorPlan {
    ordinals: HashMap<String, u16>,
    reexports: Vec<FrozenUnit>,
}

/// Assign schema-global error ordinals: locally-declared `error`s take
/// `0..N` in declaration order; each distinct `! Name` a function throws that
/// isn't local is appended as a re-exported import - resolved through the
/// in-scope `use`s when there's a project context, or a `<unresolved: Name>`
/// marker slot otherwise. Either way every throw name ends up with a stable
/// `u16`.
fn plan_error_space(
    declarations: &[rust_sitter::Spanned<Declaration>],
    use_context: Option<(&[String], &ProjectContext)>,
) -> ErrorPlan {
    let mut ordinals: HashMap<String, u16> = HashMap::new();
    let mut next: u16 = 0;

    // Locals, in declaration order. A duplicate `error` name (a validation
    // error, reported elsewhere) keeps the first slot.
    for decl in declarations {
        if let Declaration::Error(error_decl) = &decl.value {
            ordinals.entry(error_decl.name()).or_insert_with(|| {
                let ord = next;
                next += 1;
                ord
            });
        }
    }

    // Foreign errors named by a `throws`, first-reference order.
    let mut reexports: Vec<FrozenUnit> = Vec::new();
    for decl in declarations {
        let Declaration::Protocol(protocol) = &decl.value else {
            continue;
        };
        for func in protocol.functions() {
            for name in func.throws() {
                if ordinals.contains_key(&name) {
                    continue;
                }
                let ord = next;
                next += 1;
                ordinals.insert(name.clone(), ord);
                reexports.push(
                    resolve_foreign_error(&name, declarations, use_context, ord).unwrap_or_else(
                        || {
                            // It isn't a local `error` (those are already in
                            // `ordinals`) and no `use` brought a foreign one
                            // in. Either the name is declared here as
                            // something else - a `struct`, an `enum` - or it
                            // names nothing at all. The two deserve different
                            // messages, so mark which it is and let
                            // `validate` phrase it; see
                            // `NOT_AN_ERROR_PREFIX` / `UNRESOLVED_PREFIX`.
                            let marker = match local_declaration_kind(declarations, &name) {
                                Some(kind) => format!("{NOT_AN_ERROR_PREFIX}{name}: {kind}>"),
                                None => format!("{UNRESOLVED_PREFIX}{name}>"),
                            };
                            FrozenUnit::Error {
                                docstring: None,
                                parameters: vec![],
                                ordinal: ord,
                                imported_from: Some(marker),
                                name: name.clone(),
                                message: String::new(),
                                fields: vec![],
                                span: None,
                            }
                        },
                    ),
                );
            }
        }
    }

    ErrorPlan { ordinals, reexports }
}

/// Locate a foreign `error` named by a bare `! Name` throw: walk the schema's
/// `use` declarations, and for each that brings `name` into scope, look for a
/// matching `error` declaration in the target schema. `None` if there's no
/// project context, or nothing matches.
fn resolve_foreign_error(
    name: &str,
    declarations: &[rust_sitter::Spanned<Declaration>],
    use_context: Option<(&[String], &ProjectContext)>,
    ordinal: u16,
) -> Option<FrozenUnit> {
    let (current_namespace, project_context) = use_context?;
    let schema =
        find_schema_bringing_into_scope(name, declarations, current_namespace, project_context)?;

    let schema_ref = schema.borrow();
    for target_decl in &schema_ref.declarations {
        if let Declaration::Error(error_decl) = &target_decl.value {
            if error_decl.name() == name {
                return Some(frozen_error(
                    error_decl,
                    ordinal,
                    Some(schema_ref.namespace_joined()),
                    None,
                ));
            }
        }
    }

    None
}

/// Freeze one `error` declaration - shared by a local declaration and a
/// re-exported import (which only differ in `ordinal` / `imported_from`).
/// `span` is `Some` only for a local declaration - see
/// [`FrozenUnit::Error`]'s own `span` doc for why a re-export's is `None`.
fn frozen_error(
    error_decl: &grammar::Error,
    ordinal: u16,
    imported_from: Option<String>,
    span: Option<(usize, usize)>,
) -> FrozenUnit {
    let field_units: Vec<FrozenUnit> = error_decl
        .fields()
        .iter()
        .map(|field| FrozenUnit::Field {
            docstring: field.docstring(),
            parameters: annotation_units(&field.annotations()),
            optional: field.optional(),
            name: field.name(),
            ty: TypeRef::from_grammar(field.field_type()),
            default: field.default_value().and_then(Literal::from_grammar),
            span: field.span,
        })
        .collect();

    FrozenUnit::Error {
        docstring: error_decl.docstring(),
        parameters: annotation_units(&error_decl.annotations()),
        ordinal,
        imported_from,
        name: error_decl.name(),
        message: error_decl.message(),
        fields: field_units,
        span,
    }
}

/// Turn a declaration's annotations into frozen units, in source order.
///
/// A bare marker `@key` (no `=value`) becomes one `FrozenUnit::Property`
/// carrying `ConfigValue::Marker`. A scalar `@key = value` becomes the same
/// with the value's own typed form. A list `@key = [Name(k = v, ...), ...]`
/// (only `@validators` today) becomes one `FrozenUnit::ValidatorRef { name,
/// args }` per call, keeping the applied name and its keyword arguments
/// structured. `@deprecated` is the one special case: it freezes to
/// `FrozenUnit::Deprecated`, which carries its *semantics* (is-deprecated
/// plus an optional message) rather than its syntax - see
/// `deprecated_value`.
fn annotation_units(annotations: &[&Annotation]) -> Vec<FrozenUnit> {
    let mut units = Vec::new();
    for a in annotations {
        if a.key() == "deprecated" {
            units.push(FrozenUnit::Deprecated(deprecated_value(a)));
            continue;
        }
        let Some(assignment) = &a.assignment else {
            units.push(FrozenUnit::Property { name: a.key(), value: ConfigValue::Marker });
            continue;
        };
        match &assignment.value {
            AnnotationValue::Scalar(expression) => units.push(FrozenUnit::Property {
                name: a.key(),
                value: config_value_of(expression),
            }),
            AnnotationValue::List(list) => {
                for call in &list.items {
                    units.push(FrozenUnit::ValidatorRef {
                        name: call.name.text.clone(),
                        span: call.name.span,
                        arg_spans: call.args().iter().map(|arg| arg.name.span).collect(),
                        args: call
                            .args()
                            .iter()
                            .map(|arg| FrozenUnit::Property {
                                name: arg.name.text.clone(),
                                value: config_value_of(&arg.value),
                            })
                            .collect(),
                    });
                }
            }
        }
    }
    units
}

/// One grammar `Expression` to its frozen, still-typed form. The single
/// place that mapping happens, for both annotation values and validator
/// call arguments - which previously took *different* text renderings of
/// the same expression (`render` strips a string's quotes, `as_text` keeps
/// them), so the two disagreed about what a value even was.
fn config_value_of(expression: &Expression) -> ConfigValue {
    use crate::schema::idl::grammar::BoolLiteral;
    match expression {
        Expression::Bool(BoolLiteral::True) => ConfigValue::Bool(true),
        Expression::Bool(BoolLiteral::False) => ConfigValue::Bool(false),
        Expression::Integer(i) => ConfigValue::Integer(i.value),
        Expression::String(s) => ConfigValue::Str(s.value.clone()),
        Expression::Identifier(i) => ConfigValue::Name(i.text.clone()),
        Expression::Path(p) => ConfigValue::Path(p.text.clone()),
        Expression::FString(f) => ConfigValue::FString(f.source()),
    }
}

/// `@deprecated`'s value, read from the real `Expression` before it would
/// otherwise collapse to text - this is what lets `true`/`false` be told
/// apart from a same-spelled free word, instead of `deprecation_of`
/// string-comparing already-flattened text.
fn deprecated_value(annotation: &Annotation) -> crate::schema::ir::frozen::unit::DeprecatedValue {
    use crate::schema::idl::grammar::{BoolLiteral, Expression};
    use crate::schema::ir::frozen::unit::DeprecatedValue;

    let Some(assignment) = &annotation.assignment else {
        return DeprecatedValue::True; // bare marker
    };
    match &assignment.value {
        AnnotationValue::Scalar(Expression::Bool(BoolLiteral::True)) => DeprecatedValue::True,
        AnnotationValue::Scalar(Expression::Bool(BoolLiteral::False)) => DeprecatedValue::False,
        AnnotationValue::Scalar(Expression::String(s)) => DeprecatedValue::Message(s.value.clone()),
        _ => DeprecatedValue::Malformed(annotation.value().unwrap_or_default()),
    }
}

/// Marks a `! Name` that resolved to nothing at all - frozen into the
/// placeholder error's `imported_from`, which `validate` turns into a
/// diagnostic. Freeze cannot report, so it records.
pub(crate) const UNRESOLVED_PREFIX: &str = "<unresolved: ";

/// Marks a `! Name` that names a declaration in this schema which is not an
/// `error` - a `struct`, an `enum`. Only an `error` has a slot in the schema's
/// error space, so nothing else can be raised.
pub(crate) const NOT_AN_ERROR_PREFIX: &str = "<not an error: ";

/// What this schema declares `name` as, if it declares it at all - the words
/// an author would recognise, article included so a diagnostic reads
/// "is an enum" rather than "is a enum".
fn local_declaration_kind(
    declarations: &[rust_sitter::Spanned<Declaration>],
    name: &str,
) -> Option<&'static str> {
    declarations.iter().find_map(|decl| match &decl.value {
        Declaration::Struct(d) if d.name() == name => Some("a struct"),
        Declaration::Enum(d) if d.name() == name => Some("an enum"),
        Declaration::Const(d) if d.name() == name => Some("a constant"),
        Declaration::Protocol(d) if d.name() == name => Some("a protocol"),
        Declaration::Validator(d) if d.name() == name => Some("a validator"),
        Declaration::TypeAlias(d) if d.name() == name => Some("a type alias"),
        _ => None,
    })
}

/// A validator's properties as `FrozenUnit::Field`s - its own schema's
/// lowering, and what an importing schema checks a call's arguments against.
pub fn freeze_validator_properties(validator: &crate::schema::idl::grammar::Validator) -> Vec<FrozenUnit> {
    validator
        .properties()
        .iter()
        .map(|prop| FrozenUnit::Field {
            docstring: prop.docstring(),
            parameters: vec![],
            optional: false,
            name: prop.name(),
            ty: TypeRef::from_grammar(prop.property_type()),
            default: prop.default_value().and_then(Literal::from_grammar),
            span: prop.span,
        })
        .collect()
}

/// Extract path string from UsePath enum
/// TODO: This is a placeholder - should integrate with full resolver
fn extract_use_path(use_path: &crate::schema::idl::grammar::UsePath) -> String {
    use crate::schema::idl::grammar::UsePath;

    match use_path {
        UsePath::Absolute(scoped) => scoped.to_string(),
        UsePath::Relative(rel) => {
            // Convert parent::path to absolute later in resolver
            format!("{}::{}", rel.prefix.keyword(), rel.path.to_string())
        }
        UsePath::Glob(glob) => {
            format!("{}::*", glob.path.to_string())
        }
        UsePath::Multi(multi) => {
            // For now, just use the base path
            // TODO: Handle multi-imports properly
            multi.path.to_string()
        }
        UsePath::RelativeGlob(glob) => format!("{}::*", glob.prefix.keyword()),
        UsePath::RelativeMulti(multi) => multi.prefix.keyword().to_string(),
    }
}

/// Resolve a `use` declaration against the rest of the project, emitting one
/// `FrozenUnit::Import` per resolved target (multiple for a multi-import).
///
/// Only same-package targets are actually located and symbol-checked today;
/// external dependency/stdlib paths still resolve to a namespace string (via
/// `ImportResolver`) but aren't loaded/merged into this schema's context yet.
fn resolve_use_declaration(
    project_context: &ProjectContext,
    current_namespace: &[String],
    use_path: &UsePath,
    alias: Option<String>,
    span: (usize, usize),
) -> Vec<FrozenUnit> {
    let resolver = ImportResolver::new(vec![], Default::default(), None);

    let target = match resolve_use_to_schema(project_context, &resolver, current_namespace, use_path) {
        Ok(target) => target,
        Err(message) => {
            tracing::warn!("Failed to resolve use path: {}", message);
            return vec![FrozenUnit::Import(format!("<unresolved: {}>", message), alias, span)];
        }
    };

    let joined_namespace = target.resolved.absolute_namespace.join("::");

    // Glob import: `use ns::*;` — an alias here is meaningless.
    if target.resolved.symbols == ["*".to_string()] {
        let mut units = vec![FrozenUnit::Import(format!("{}::*", joined_namespace), None, span)];
        if let Some(schema) = &target.schema {
            units.extend(declared_symbol_names(&schema.borrow()).into_iter().map(|name| {
                FrozenUnit::Import(format!("{}::{}", joined_namespace, name), None, span)
            }));
        }
        return units;
    }

    // Item imports: `use ns::{A, B};` — an alias here is meaningless.
    if !target.resolved.symbols.is_empty() {
        return target
            .resolved
            .symbols
            .iter()
            .map(|item| {
                if !matches!(&target.schema, Some(schema) if importable_names(&schema.borrow()).contains(item))
                {
                    tracing::warn!("Symbol '{}' not found in schema '{}'", item, joined_namespace);
                }
                FrozenUnit::Import(format!("{}::{}", joined_namespace, item), None, span)
            })
            .collect();
    }

    // Whole-namespace or single-symbol import (`use ns;` / `use ns::Symbol;`).
    // The alias (`use ... as X`) binds `X` to that one target.
    match &target.schema {
        Some(schema) if target.remaining.is_empty() => {
            let ns = schema.borrow().namespace_joined();
            let mut units = vec![FrozenUnit::Import(ns.clone(), alias, span)];
            units.extend(
                declared_symbol_names(&schema.borrow())
                    .into_iter()
                    .map(|name| FrozenUnit::Import(format!("{}::{}", ns, name), None, span)),
            );
            units
        }
        Some(schema) => {
            let symbol = target.remaining.join("::");
            let schema_namespace = schema.borrow().namespace_joined();

            if !importable_names(&schema.borrow()).contains(&symbol) {
                tracing::warn!("Symbol '{}' not found in schema '{}'", symbol, schema_namespace);
            }

            vec![FrozenUnit::Import(
                format!("{}::{}", schema_namespace, symbol),
                alias,
                span,
            )]
        }
        // Not part of this project (external dependency, stdlib, or genuinely
        // unresolved) - fall back to the raw resolved namespace.
        None => vec![FrozenUnit::Import(joined_namespace, alias, span)],
    }
}
