// Standard Uses
use std::collections::{HashMap, HashSet};

// Crate Uses
use crate::package::config::ir::context::ProjectContext;
use crate::schema::idl::grammar::Declaration;
use crate::schema::ir::compiler::alias_resolution::check_aliases;
use crate::schema::ir::compiler::import_resolver::{
    check_imports, resolve_use_to_schema, ImportResolver, ProjectImports,
};
use crate::schema::ir::compiler::interpreter::IncrementalInterpreter;
use crate::schema::ir::compiler::settings::enforcement::check_settings_enforcement;
use crate::schema::ir::compiler::settings::resolution::check_settings_conflicts;
use crate::schema::ir::compiler::settings::severity::resolve_diagnostic_overrides;
use crate::schema::ir::frozen::unit::FrozenUnit;
use crate::schema::ir::validation;
use crate::schema::idl::annotations::DEFAULT_HANDSHAKE;
use crate::schema::ir::frozen::unit::ConfigValue;
use crate::settings::effective::package_settings;

// External Uses
use eyre::{bail, Result};

pub fn interpret_context(project_context: &ProjectContext) -> Result<()> {
    if let Some(cycle) = detect_import_cycle(project_context) {
        bail!("Import cycle detected between schemas: {}", cycle.join(" -> "));
    }

    let mut schema_errors: Vec<String> = vec![];
    let package_settings_dict = package_settings(project_context);
    let package_handshake = project_context
        .config_frozen
        .as_deref()
        .map(crate::package::config::ir::frozen::handshake)
        .unwrap_or(DEFAULT_HANDSHAKE)
        .to_string();
    // Package-level (`.idp`) settings origins aren't span-tracked yet
    // (only schema-level `settings { ... }` blocks are) - a malformed
    // package-level value falls back to usage-site-only diagnostics, same
    // as before this change.
    let package_settings_spans = crate::settings::SettingsSpans::default();

    for schema_context in project_context.schema_contexts.iter() {
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
        apply_package_handshake(&mut frozen_units, &package_handshake);

        let imports = ProjectImports {
            declarations: &declarations,
            namespace: &namespace,
            project: project_context,
        };
        match validation::validate_with(&frozen_units, &imports) {
            Ok(warnings) => {
                let (warnings, fatal) = resolve_diagnostic_overrides(
                    &frozen_units, warnings, &package_settings_dict, &package_settings_spans,
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

        if let Err(errors) =
            check_settings_enforcement(&frozen_units, &package_settings_dict, &package_settings_spans)
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
    }

    if !schema_errors.is_empty() {
        bail!("{}", schema_errors.join("\n\n"));
    }

    Ok(())
}

/// Build a same-project schema dependency graph from `use` declarations and
/// look for cycles (e.g. schema A `use`s something from B, and B `use`s
/// something from A). Returns the offending cycle's namespaces, if any.
fn detect_import_cycle(project_context: &ProjectContext) -> Option<Vec<String>> {
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

/// Freeze the package's default handshake into every protocol that doesn't
/// name its own, so the choice is part of each protocol's frozen IR (and its
/// hash): changing the package default then shows up as a change to the
/// protocols it reaches. `comline@1` is what an absent `@handshake` already
/// means, so it's left implicit and existing IR hashes don't move.
fn apply_package_handshake(units: &mut [FrozenUnit], package_handshake: &str) {
    if package_handshake == DEFAULT_HANDSHAKE {
        return;
    }
    for unit in units {
        let FrozenUnit::Protocol { parameters, .. } = unit else {
            continue;
        };
        let declared = parameters
            .iter()
            .any(|p| matches!(p, FrozenUnit::Property { name, .. } if name == "handshake"));
        if !declared {
            parameters.push(FrozenUnit::Property {
                name: "handshake".to_string(),
                value: ConfigValue::Str(package_handshake.to_string()),
            });
        }
    }
}
