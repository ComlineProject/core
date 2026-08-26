// Standard Uses
use std::collections::{HashMap, HashSet};

// Crate Uses
use crate::package::config::ir::context::ProjectContext;
use crate::schema::idl::grammar::Declaration;
use crate::schema::ir::compiler::import_resolver::{resolve_use_to_schema, ImportResolver};
use crate::schema::ir::compiler::interpreter::IncrementalInterpreter;
use crate::schema::ir::frozen::unit::FrozenUnit;

// External Uses
use eyre::{bail, Result};

pub fn interpret_context(project_context: &ProjectContext) -> Result<()> {
    if let Some(cycle) = detect_import_cycle(project_context) {
        bail!("Import cycle detected between schemas: {}", cycle.join(" -> "));
    }

    for schema_context in project_context.schema_contexts.iter() {
        let declarations = { schema_context.borrow().declarations.clone() };
        let namespace = { schema_context.borrow().namespace.clone() };

        let mut frozen_units = IncrementalInterpreter::from_declarations_with_context(
            declarations,
            &namespace,
            project_context,
        );

        // Inject Namespace unit
        let namespace_joined = schema_context.borrow().namespace_joined();
        frozen_units.insert(0, FrozenUnit::Namespace(namespace_joined));

        *schema_context.borrow().frozen_schema.borrow_mut() = Some(frozen_units);
        schema_context.borrow().compile_state.borrow_mut().complete = true;
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
