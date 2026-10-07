// Standard Uses
#[cfg(feature = "deps")]
use std::collections::HashSet;
#[cfg(feature = "deps")]
use std::path::{Path, PathBuf};

// Crate Uses
use crate::package::config::ir::context::ProjectContext;
use crate::package::config::ir::frozen::FrozenUnit;
use crate::package::config::ir::interpreter::freezing;
#[cfg(feature = "deps")]
use crate::package::config::idl::grammar::{Key, Value};
use crate::settings::SettingsDict;

// External Uses


/// `cross_package_override`, when `Some`, is a `settings = <dep>::settings::
/// <name>` reference already resolved to its real content by
/// [`resolve_cross_package_settings`] — the `settings` assignment uses it
/// directly instead of interpreting its own (irrelevant, in that case)
/// `Value`. `None` for every in-memory/no-filesystem caller (there's
/// nothing to resolve a reference against) and for a package whose
/// `settings` is a literal dictionary or absent.
#[allow(unused)]
pub fn interpret_context(
    context: &ProjectContext,
    cross_package_override: Option<&SettingsDict>,
) -> Result<Vec<FrozenUnit>, Box<dyn snafu::Error>>
{
    let mut interpreted = vec![];

    for assignment in &context.config.assignments {
        // let file = context.config.0.files().first().unwrap();
        // let span = file.range_of(node.0).unwrap();

        interpreted.append(
            &mut freezing::interpret_node_into_frozen(context, assignment, cross_package_override)?
        );
    }

    // `settings` is optional - a package that writes none still gets a
    // (currently empty) Comline-provided default, the same "sensible
    // default, override when you need to" shape `specification_version`
    // and `publish_registries` are heading toward. No other `.idp` field
    // has this absent-key-gets-a-default behavior yet; this is the first.
    if !interpreted.iter().any(|u| matches!(u, FrozenUnit::Settings(_))) {
        interpreted.push(FrozenUnit::Settings(crate::settings::SettingsDict::default()));
    }

    Ok(interpreted)
    // freezing::into_frozen_whole(&context, interpreted)
}

/// Resolves a `settings = <dep>::settings::<name>` reference — decision 5 of
/// the settings design, cross-package sharing: a package adopting another
/// package's named `settings Foo { ... }` schema block as its own default.
/// `Ok(None)` when there's nothing to resolve (no `settings` assignment, or
/// one that isn't a `::`-namespaced reference — a literal dictionary or
/// absent `settings` is [`interpret_context`]'s own concern, unchanged).
///
/// Filesystem-only (needs `project_root` to locate the dependency, hence
/// `feature = "deps"`): the in-memory [`crate::package::build::PackageSources`]
/// path has no dependencies at all, so a reference there is a plain error
/// from [`interpret_context`]'s own fallback, not routed through here.
///
/// `in_progress` is the same cycle-guard set threaded through
/// [`crate::package::deps::resolve_with`] — resolving this reference
/// recursively compiles the referenced dependency exactly like a normal
/// schema-level dependency would, so it shares the same guard.
#[cfg(feature = "deps")]
pub(crate) fn resolve_cross_package_settings(
    context: &ProjectContext,
    project_root: &Path,
    in_progress: &mut HashSet<PathBuf>,
) -> Result<Option<SettingsDict>, String> {
    let Some(assignment) = context.config.assignments.iter().find_map(|a| {
        let a = &a.value;
        matches!(&a.key, Key::Identifier(id) if id.value == "settings").then_some(a)
    }) else {
        return Ok(None);
    };

    let Value::Namespaced(ns) = &assignment.value else {
        return Ok(None);
    };

    let segments: Vec<&str> = ns.value.split("::").collect();
    let [dep_name, "settings", block_name] = segments.as_slice() else {
        return Err(format!(
            "settings = {}: a cross-package reference must have the shape \
             <dependency>::settings::<name>",
            ns.value
        ));
    };

    let dependencies =
        crate::package::config::dependency::DependencyConfig::parse_dependencies(
            &context.config.assignments,
        )?;
    let dep = dependencies.get(*dep_name).ok_or_else(|| {
        format!("settings = {}: no dependency named `{dep_name}` is declared", ns.value)
    })?;

    let cache_dir = project_root.join(crate::package::config::dependency::DEPS_CACHE_DIR);
    let resolved = crate::package::deps::resolve_with(dep, project_root, &cache_dir, in_progress)
        .map_err(|e| format!("settings = {}: {e}", ns.value))?;

    match find_named_settings_block(&resolved.context, block_name)? {
        Some(dict) => Ok(Some(dict)),
        None => Err(format!(
            "settings = {}: package `{dep_name}` has no settings block named `{block_name}`",
            ns.value
        )),
    }
}

/// Every schema in a (already compiled) dependency, scanned for a named
/// `settings <name> { ... }` block. `Ok(None)`: no match. `Err`: more than
/// one schema defines the same name — ambiguous, naming both, same
/// philosophy as a duplicate dotted-key within one block already being a
/// hard error rather than silently picking one.
#[cfg(feature = "deps")]
fn find_named_settings_block(
    dep_context: &ProjectContext,
    name: &str,
) -> Result<Option<SettingsDict>, String> {
    use crate::schema::ir::frozen::unit::FrozenUnit as SchemaFrozenUnit;

    let mut found: Option<(SettingsDict, String)> = None;
    for schema in &dep_context.schema_contexts {
        let schema_ref = schema.borrow();
        let frozen_ref = schema_ref.frozen_schema.borrow();
        let Some(units) = frozen_ref.as_ref() else { continue };
        for unit in units {
            let SchemaFrozenUnit::Settings { name: Some(n), values, .. } = unit else { continue };
            if n != name {
                continue;
            }
            let this_namespace = schema_ref.namespace.join("::");
            if let Some((_, first_namespace)) = &found {
                return Err(format!(
                    "settings block `{name}` is ambiguous: found in both `{first_namespace}` \
                     and `{this_namespace}`"
                ));
            }
            found = Some((values.clone(), this_namespace));
        }
    }
    Ok(found.map(|(dict, _)| dict))
}
