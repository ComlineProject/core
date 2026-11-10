// Standard Uses
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

// Crate Uses
use crate::package::config::ir::context::ProjectContext;
use crate::schema::idl::grammar::Declaration;
use crate::schema::ir::compiler::alias_resolution::check_aliases;
use crate::schema::ir::compiler::import_resolver::{
    check_imports, resolve_use_to_schema, use_brings_into_scope, ImportResolver, ProjectImports,
};
use crate::schema::ir::compiler::interpreter::IncrementalInterpreter;
use crate::schema::ir::compiler::settings::enforcement::check_settings_enforcement;
use crate::schema::ir::compiler::settings::resolution::check_settings_conflicts;
use crate::schema::ir::compiler::settings::severity::resolve_diagnostic_overrides;
use crate::schema::ir::context::SchemaContext;
use crate::schema::ir::frozen::unit::FrozenUnit;
use crate::schema::ir::validation;
use crate::schema::ir::package_wire::PackageWire;
use crate::settings::effective::package_settings;

// External Uses
use eyre::{bail, Result};

pub fn interpret_context(project_context: &ProjectContext) -> Result<()> {
    if let Some(cycle) = detect_import_cycle(project_context) {
        bail!("Import cycle detected between schemas: {}", cycle.join(" -> "));
    }

    let mut schema_errors: Vec<String> = vec![];
    let own_settings = package_settings(project_context);
    let own_config = project_context.config_frozen.as_deref().unwrap_or(&[]);
    let (own_format, _) = crate::package::config::ir::frozen::wire(own_config);
    let own_format = own_format.map(str::to_string);
    // The package's framing, and its handshake: with none stated, one for
    // all the package's protocols, as they share a connection: `comline@2`
    // where they all can run it.
    let own_protocols: Vec<Vec<FrozenUnit>> = project_context
        .schema_contexts
        .iter()
        .filter(|s| !s.borrow().compile_state.borrow().complete)
        .flat_map(|s| {
            s.borrow()
                .declarations
                .iter()
                .filter_map(|d| match &d.value {
                    Declaration::Protocol(p) => Some(
                        crate::schema::ir::compiler::interpreter::incremental::annotation_units(&p.annotations()),
                    ),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let wire = PackageWire::of(own_config, own_protocols.iter().map(Vec::as_slice));
    check_package_encodings(project_context, own_format.as_deref(), &wire.framing, &wire.handshake)?;
    // What the package says of what its own settings give up, for every
    // protocol that takes them.
    let package_unverified = crate::package::config::ir::frozen::unverified(own_config);
    if let Some(package_unverified) = package_unverified {
        check_package_unverified(package_unverified, &own_settings)?;
    }
    let mut package_covered = vec![];
    let mut first_own: Option<Rc<RefCell<SchemaContext>>> = None;
    // Package-level (`.idp`) settings origins aren't span-tracked yet
    // (only schema-level `settings { ... }` blocks are) - a malformed
    // package-level value falls back to usage-site-only diagnostics, same
    // as before this change.
    let package_settings_spans = crate::settings::SettingsSpans::default();

    // Imported schemas first: a schema using another's types reads what
    // that one's compile found unbounded.
    for schema_context in compile_order(project_context).iter() {
        // A dependency's schemas arrive compiled by its own package (its
        // settings, its default handshake); they're only resolved against.
        if schema_context.borrow().compile_state.borrow().complete {
            continue;
        }
        let declarations = { schema_context.borrow().declarations.clone() };
        let namespace = { schema_context.borrow().namespace.clone() };

        if let Err(errors) = check_imports(&declarations, &namespace, project_context) {
            let rendered: Vec<String> = errors
                .iter()
                .map(|error| schema_context.borrow().render(error))
                .collect();
            schema_errors.push(format!(
                "Schema '{}' has unresolved imports:\n{}",
                namespace.join("::"),
                rendered.join("\n\n")
            ));
            continue;
        }

        if let Err(errors) = check_aliases(&declarations, Some((&namespace, project_context))) {
            let rendered: Vec<String> = errors
                .iter()
                .map(|error| schema_context.borrow().render(error))
                .collect();
            schema_errors.push(format!(
                "Schema '{}' failed alias resolution:\n{}",
                namespace.join("::"),
                rendered.join("\n\n")
            ));
            continue;
        }

        if let Err(errors) = crate::schema::ir::compiler::bounds::check_bounds(
            &declarations,
            Some((&namespace, project_context)),
        ) {
            let rendered: Vec<String> = errors
                .iter()
                .map(|error| schema_context.borrow().render(error))
                .collect();
            schema_errors.push(format!(
                "Schema '{}' has invalid sizes:\n{}",
                namespace.join("::"),
                rendered.join("\n\n")
            ));
            continue;
        }

        if let Err(errors) = check_settings_conflicts(&declarations) {
            let rendered: Vec<String> = errors
                .iter()
                .map(|error| schema_context.borrow().render(error))
                .collect();
            schema_errors.push(format!(
                "Schema '{}' has invalid settings:\n{}",
                namespace.join("::"),
                rendered.join("\n\n")
            ));
            continue;
        }

        let mut frozen_units = IncrementalInterpreter::from_declarations_with_context(
            declarations.clone(),
            &namespace,
            project_context,
        );

        // Inject Namespace unit
        let namespace_joined = schema_context.borrow().namespace_joined();
        frozen_units.insert(0, FrozenUnit::Namespace(namespace_joined.clone()));
        // What each protocol takes from the package is read before the
        // package's handshake is frozen in, which makes it look its own.
        let inherits = crate::schema::ir::unverified::inherits(&frozen_units);
        wire.freeze_handshake(&mut frozen_units);
        crate::schema::ir::unverified::freeze_with(
            &mut frozen_units,
            &crate::schema::ir::unverified::Encodings::of(project_context),
            package_unverified,
            &inherits,
        );
        package_covered.extend(crate::schema::ir::unverified::package_covered_by(&frozen_units));
        // Settings gate what the schema's author wrote; the package's
        // framing is the package's choice, so it's added after the snapshot
        // enforcement checks.
        let written_units = frozen_units.clone();
        wire.freeze_framing(&mut frozen_units);
        // Default bounds, from the package's settings and the schema's
        // (core#98), stored resolved at each position.
        crate::schema::ir::compiler::bounds::apply_default_bounds(&mut frozen_units, &own_settings);
        let imported =
            |name: &str| imports_unbounded(name, &declarations, &namespace, project_context);
        let unbounded =
            crate::schema::ir::compiler::bounds::check_bounded(&mut frozen_units, &imported);
        let exported =
            crate::schema::ir::compiler::bounds::unbounded_declarations(&frozen_units, &imported);
        *schema_context.borrow().unbounded.borrow_mut() = Some(exported);
        if !unbounded.is_empty() {
            let rendered: Vec<String> =
                unbounded.iter().map(|error| schema_context.borrow().render(error)).collect();
            schema_errors.push(format!(
                "Schema '{}' has values without a maximum:\n{}",
                namespace_joined,
                rendered.join("\n\n")
            ));
        }

        let imports = ProjectImports {
            declarations: &declarations,
            namespace: &namespace,
            project: project_context,
        };
        match validation::validate_with(&frozen_units, &imports) {
            Ok(warnings) => {
                let (warnings, fatal) = resolve_diagnostic_overrides(
                    &frozen_units, warnings, &own_settings, &package_settings_spans,
                );
                schema_context.borrow().warnings.borrow_mut().extend(warnings);
                if !fatal.is_empty() {
                    let rendered: Vec<String> = fatal
                        .iter()
                        .map(|error| schema_context.borrow().render(error))
                        .collect();
                    schema_errors.push(format!(
                        "Schema '{}' has a diagnostic escalated to a build failure:\n{}",
                        namespace_joined,
                        rendered.join("\n\n")
                    ));
                }
            }
            Err(errors) => {
                let rendered: Vec<String> = errors
                    .iter()
                    .map(|error| schema_context.borrow().render(error))
                    .collect();
                schema_errors.push(format!(
                    "Schema '{}' failed validation:\n{}",
                    namespace_joined,
                    rendered.join("\n\n")
                ));
            }
        }

        // Words the package's edition deprecates or removes (core#102). Only
        // the package's own schemas get here: a dependency's arrive compiled
        // by its own package, under its own edition.
        let source = schema_context.borrow().codemap.files().first().map(|f| f.contents().to_string());
        if let Some(source) = source {
            let (removed, deprecated): (Vec<_>, Vec<_>) = crate::edition::deprecations(
                &source,
                project_context.edition(),
                crate::schema::idl::vocabulary::DEPRECATIONS,
            )
            .into_iter()
            .partition(|d| d.code == crate::edition::REMOVED);
            let (deprecated, fatal) = resolve_diagnostic_overrides(
                &frozen_units, deprecated, &own_settings, &package_settings_spans,
            );
            schema_context.borrow().warnings.borrow_mut().extend(deprecated);
            let failing: Vec<String> = removed
                .iter()
                .chain(&fatal)
                .map(|error| schema_context.borrow().render(error))
                .collect();
            if !failing.is_empty() {
                schema_errors.push(format!(
                    "Schema '{}' uses words its edition removed:\n{}",
                    namespace_joined,
                    failing.join("\n\n")
                ));
            }
        }

        if let Err(errors) =
            check_settings_enforcement(&written_units, &own_settings, &package_settings_spans)
        {
            let rendered: Vec<String> = errors
                .iter()
                .map(|error| schema_context.borrow().render(error))
                .collect();
            schema_errors.push(format!(
                "Schema '{}' failed settings enforcement:\n{}",
                namespace_joined,
                rendered.join("\n\n")
            ));
        }

        *schema_context.borrow().frozen_schema.borrow_mut() = Some(frozen_units);
        schema_context.borrow().compile_state.borrow_mut().complete = true;
        first_own.get_or_insert_with(|| schema_context.clone());
    }

    // The package's `unverified` lists what none of its protocols give up
    // through its settings: a warning, on the first schema it compiled, as it
    // has no schema of its own to be on.
    if let (Some(package_unverified), Some(first), true) = (package_unverified, &first_own, schema_errors.is_empty()) {
        package_covered.sort();
        package_covered.dedup();
        let unused = validation::validator::check_package_unverified_used(package_unverified, &package_covered);
        let frozen = first.borrow().frozen_schema.borrow().clone().unwrap_or_default();
        let (warnings, fatal) =
            resolve_diagnostic_overrides(&frozen, unused, &own_settings, &package_settings_spans);
        first.borrow().warnings.borrow_mut().extend(warnings);
        schema_errors.extend(fatal.iter().map(|d| crate::diagnostics::render(d, "config.idp", "")));
    }

    if !schema_errors.is_empty() {
        bail!("{}", schema_errors.join("\n\n"));
    }

    Ok(())
}

/// The package's own `wire` and `handshake` names, once its dependencies
/// are known: a namespaced one has to be declared, by this package or a
/// dependency. Official ones were checked when the config froze.
fn check_package_encodings(
    project_context: &ProjectContext,
    format: Option<&str>,
    framing: &str,
    handshake: &str,
) -> Result<()> {
    use crate::encodings::{is_declared, undeclared, EncodingKind};

    let names = [
        (EncodingKind::Format, format, "wire.format"),
        (EncodingKind::Framing, Some(framing), "wire.framing"),
        (EncodingKind::Handshake, Some(handshake), "handshake"),
    ];
    for (kind, name, key) in names {
        let Some(name) = name else { continue };
        if !is_declared(project_context, kind, name) {
            bail!("config.idp's `{key}`: {}", undeclared(kind, name));
        }
    }
    Ok(())
}

/// The package's `unverified`, judged without its protocols: allowed by the
/// package's settings as `@unverified` is, and its keys and reasons sound.
fn check_package_unverified(
    package_unverified: &crate::schema::ir::frozen::unit::ConfigValue,
    package_settings: &crate::settings::SettingsDict,
) -> Result<()> {
    let mut problems = crate::schema::ir::compiler::settings::enforcement::check_package_annotation(
        "unverified",
        package_settings,
        &crate::settings::SettingsSpans::default(),
        "config.idp's `unverified` is held to the same gate as `@unverified`",
    )
    .err()
    .unwrap_or_default();
    problems.extend(validation::validator::check_package_unverified(package_unverified));
    if problems.is_empty() {
        return Ok(());
    }
    let rendered: Vec<String> = problems.iter().map(|d| crate::diagnostics::render(d, "config.idp", "")).collect();
    bail!("config.idp's `unverified` is invalid:\n{}", rendered.join("\n\n"))
}

/// Whether `name`, as a schema with these `declarations` uses it, is a
/// type another schema declares that holds a value without a maximum
/// ([`SchemaContext::unbounded`]). Every schema a `use` could bring it in
/// from is asked, and one not yet compiled answers yes: it fails closed.
///
/// [`SchemaContext::unbounded`]: crate::schema::ir::context::SchemaContext::unbounded
fn imports_unbounded(
    name: &str,
    declarations: &[rust_sitter::Spanned<Declaration>],
    namespace: &[String],
    project_context: &ProjectContext,
) -> bool {
    let resolver = ImportResolver::new(vec![], Default::default(), None);
    let (qualifier, bare) = match name.rsplit_once("::") {
        Some((qualifier, bare)) => (Some(qualifier), bare),
        None => (None, name),
    };
    let declares = |schema: &Rc<RefCell<SchemaContext>>, symbol: &str| {
        let schema = schema.borrow();
        if schema.namespace == namespace {
            return false;
        }
        let known = schema
            .unbounded
            .borrow()
            .as_ref()
            .map(|unbounded| unbounded.contains(symbol));
        known.unwrap_or(true)
    };

    // A qualified name may name its schema outright.
    if let Some(qualifier) = qualifier {
        let parts: Vec<String> = qualifier.split("::").map(str::to_string).collect();
        if let Some((schema, remaining)) =
            project_context.find_schema_by_import_namespace_parts(&parts)
        {
            if remaining.is_empty() && declares(&schema, bare) {
                return true;
            }
        }
    }
    declarations.iter().any(|decl| {
        let Declaration::Use(use_stmt) = &decl.value else {
            return false;
        };
        let Ok(target) =
            resolve_use_to_schema(project_context, &resolver, namespace, &use_stmt.path)
        else {
            return false;
        };
        let Some(schema) = &target.schema else {
            return false;
        };
        let alias = use_stmt.alias.as_ref().map(|a| a.name.text.as_str());
        if !use_brings_into_scope(&target.resolved, &target.remaining, alias, bare) {
            return false;
        }
        // `use ns::Name as X` brings `Name` in as `X`.
        let symbol = match (alias, target.remaining.last()) {
            (Some(alias), Some(symbol)) if alias == bare => symbol.as_str(),
            _ => bare,
        };
        declares(schema, symbol)
    })
}

/// The schemas to compile, each after the ones it imports, otherwise in
/// the order they were added.
fn compile_order(project_context: &ProjectContext) -> Vec<Rc<RefCell<SchemaContext>>> {
    let edges = schema_imports(project_context);
    let by_namespace: HashMap<String, &Rc<RefCell<SchemaContext>>> = project_context
        .schema_contexts
        .iter()
        .map(|s| (s.borrow().namespace_joined(), s))
        .collect();
    fn visit<'a>(
        node: &'a str,
        edges: &'a HashMap<String, HashSet<String>>,
        placed: &mut HashSet<&'a str>,
        order: &mut Vec<&'a str>,
    ) {
        if !placed.insert(node) {
            return;
        }
        let mut imports: Vec<&String> = edges.get(node).into_iter().flatten().collect();
        imports.sort();
        for import in imports {
            visit(import, edges, placed, order);
        }
        order.push(node);
    }
    let namespaces: Vec<String> = project_context
        .schema_contexts
        .iter()
        .map(|s| s.borrow().namespace_joined())
        .collect();
    let mut placed = HashSet::new();
    let mut order = Vec::new();
    for namespace in &namespaces {
        visit(namespace, &edges, &mut placed, &mut order);
    }
    order
        .into_iter()
        .filter_map(|n| by_namespace.get(n).map(|s| Rc::clone(s)))
        .collect()
}

/// Build a same-project schema dependency graph from `use` declarations and
/// look for cycles (e.g. schema A `use`s something from B, and B `use`s
/// something from A). Returns the offending cycle's namespaces, if any.
fn detect_import_cycle(project_context: &ProjectContext) -> Option<Vec<String>> {
    let edges = schema_imports(project_context);

    let mut visited = HashSet::new();
    let mut visiting = HashSet::new();
    let mut path = Vec::new();

    for node in edges.keys() {
        if !visited.contains(node) {
            if let Some(cycle) = visit_for_cycle(node, &edges, &mut visited, &mut visiting, &mut path) {
                return Some(cycle);
            }
        }
    }

    None
}

/// Each schema's namespace, and the namespaces of the project's schemas
/// its `use` declarations import.
fn schema_imports(project_context: &ProjectContext) -> HashMap<String, HashSet<String>> {
    let resolver = ImportResolver::new(vec![], Default::default(), None);
    let mut edges: HashMap<String, HashSet<String>> = HashMap::new();

    for schema_context in project_context.schema_contexts.iter() {
        let schema_context = schema_context.borrow();
        let from_namespace = schema_context.namespace_joined();
        let mut depends_on = HashSet::new();

        for decl in &schema_context.declarations {
            if let Declaration::Use(use_stmt) = &decl.value {
                let Ok(target) = resolve_use_to_schema(
                    project_context,
                    &resolver,
                    &schema_context.namespace,
                    &use_stmt.path,
                ) else {
                    continue;
                };

                if let Some(target_schema) = target.schema {
                    let to_namespace = target_schema.borrow().namespace_joined();
                    if to_namespace != from_namespace {
                        depends_on.insert(to_namespace);
                    }
                }
            }
        }

        edges.insert(from_namespace, depends_on);
    }

    edges
}

fn visit_for_cycle(
    node: &str,
    edges: &HashMap<String, HashSet<String>>,
    visited: &mut HashSet<String>,
    visiting: &mut HashSet<String>,
    path: &mut Vec<String>,
) -> Option<Vec<String>> {
    if visiting.contains(node) {
        let start = path.iter().position(|n| n == node).unwrap_or(0);
        let mut cycle = path[start..].to_vec();
        cycle.push(node.to_string());
        return Some(cycle);
    }
    if visited.contains(node) {
        return None;
    }

    visiting.insert(node.to_string());
    path.push(node.to_string());

    if let Some(targets) = edges.get(node) {
        for target in targets {
            if let Some(cycle) = visit_for_cycle(target, edges, visited, visiting, path) {
                return Some(cycle);
            }
        }
    }

    path.pop();
    visiting.remove(node);
    visited.insert(node.to_string());

    None
}
