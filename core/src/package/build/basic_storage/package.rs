// Standard Uses
use std::path::Path;

// Crate Uses
use crate::package::config::ir::context::ProjectContext;
use crate::package::config::ir::frozen::{
    basic_storage as basic_storage_project, FrozenUnit as ProjectFrozenUnit, MINIMUM_VERSION,
};
use crate::schema::ir::frozen::{
    basic_storage as basic_storage_schema, unit::FrozenUnit as SchemaFrozenUnit,
};

// External Uses
use eyre::{Context, Result};

pub(crate) fn freeze_project(
    latest_project_ctx: &ProjectContext,
    package_path: &Path,
) -> Result<()> {
    let frozen_path = package_path.join(".frozen/");

    if frozen_path.exists() {
        std::fs::remove_dir_all(&frozen_path)?
    }
    std::fs::create_dir(&frozen_path)?;

    let dependencies_path = frozen_path.join("dependencies/");
    std::fs::create_dir(&dependencies_path)?;

    let frozen_package_path = frozen_path.join("package/");
    std::fs::create_dir(&frozen_package_path)?;

    let index_path = frozen_package_path.join("index");
    std::fs::write(index_path, MINIMUM_VERSION)?;

    let versions_path = frozen_package_path.join("versions/");
    std::fs::create_dir(&versions_path)?;

    let version_path = versions_path.join(MINIMUM_VERSION.to_owned() + "/");
    std::fs::create_dir(&version_path)?;

    let config_path = version_path.join("config");
    let frozen_project_processed = basic_storage_project::serialize::to_processed(
        latest_project_ctx.config_frozen.as_ref().unwrap(),
    );
    std::fs::write(config_path, frozen_project_processed)?;

    let schemas_path = version_path.join("schemas/");
    std::fs::create_dir(&schemas_path)?;

    for schema_ctx in &latest_project_ctx.schema_contexts {
        let schema_ref = schema_ctx.borrow();
        let compile_state = schema_ref.compile_state.borrow();

        if !compile_state.complete {
            panic!("Cannot freeze schema because it is not marked as compiled")
        }

        let frozen_meta = basic_storage_schema::serialize::to_processed(
            schema_ref.frozen_schema.borrow().as_ref().unwrap(),
        );
        let schema_path = schemas_path.join(&schema_ref.namespace_joined());

        std::fs::write(schema_path, frozen_meta)?;
    }

    Ok(())
}

use std::collections::{HashMap, HashSet};
// use std::cmp::max; // Unused for now

// ... VersionBump enum ...

pub(crate) fn freeze_and_compare_packages(
    _previous_project: &[ProjectFrozenUnit],
    previous_schemas: &[Vec<SchemaFrozenUnit>],
    latest_project_ctx: &ProjectContext,
    latest_version_path: &Path,
) -> Result<()> {
    // Collect latest schemas
    let mut latest_schemas = vec![];
    for schema_ctx in &latest_project_ctx.schema_contexts {
        let schema_ref = schema_ctx.borrow();
        let frozen_ref = schema_ref.frozen_schema.borrow();
        if let Some(frozen) = frozen_ref.as_ref() {
            latest_schemas.push(frozen.clone());
        }
    }

    // Calculate version bump (unused here, logic moved to caller, but we keep the call check or just ignore result?)
    // The caller ALREADY did this to determine the version path.
    // So strictly we don't *need* to call check_difference here again unless we want to double check?
    // Let's just remove the call or silence the variable to avoid double work/logs.
    // Or better, let's keep logic if we move this responsibility here later.
    // For now, silencing to fix warning.
    let _bump = check_difference(previous_schemas, &latest_schemas);

    // ... existing freeze logic ...

    // Determine new version
    // This function doesn't actually determine the path from the bump,
    // the caller (basic_storage/mod.rs) determined the path.
    // We should probably return the bump or verifying the path matches the bump?
    // For now, let's just log it or expect the caller to handle versioning.
    // Wait, the caller 'process_changes' calls this.
    // The previous implementation of 'process_changes' blindly bumped Minor.
    // We should move the version calculation UP to the caller, or do it here and return the utilized version?

    // The current signature receives `latest_version_path`.
    // This implies the version is already decided.
    // We should change the architecture so `process_changes` calls `check_difference` FIRST,
    // decides the version, then calls `freeze_and_compare` (maybe rename to just `freeze`).

    // Let's stick to the current task: implementing logic.
    // I will write the freeze logic as requested.

    let config_path = latest_version_path.join("config");
    let frozen_project_processed = basic_storage_project::serialize::to_processed(
        latest_project_ctx.config_frozen.as_ref().unwrap(),
    );
    std::fs::write(config_path, frozen_project_processed)?;

    let schemas_path = latest_version_path.join("schemas");
    std::fs::create_dir_all(&schemas_path).with_context(|| {
        format!(
            "Could not create frozen schemas directory at '{}'",
            schemas_path.display()
        )
    })?;

    for schema_ctx in &latest_project_ctx.schema_contexts {
        let schema_ref = schema_ctx.borrow();
        let compile_state = schema_ref.compile_state.borrow();

        if !compile_state.complete {
            panic!("Cannot freeze schema because it is not marked as compiled")
        }

        let frozen_meta = basic_storage_schema::serialize::to_processed(
            schema_ref.frozen_schema.borrow().as_ref().unwrap(),
        );

        let schema_path = schemas_path.join(&schema_ref.namespace_as_path());

        std::fs::create_dir_all(&schema_path.parent().unwrap()).with_context(|| {
            format!(
                "Could not create frozen schema parent directories at '{}",
                schema_path.parent().unwrap().display()
            )
        })?;

        std::fs::write(&schema_path, frozen_meta).with_context(|| {
            format!(
                "Could not write frozen schema to path '{}'",
                schema_path.display()
            )
        })?;
    }

    Ok(())
}

// use std::collections::{HashMap, HashSet};
// use std::cmp::max;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VersionBump {
    None,
    Patch,
    Minor,
    Major,
}

pub fn check_difference(
    previous_schemas: &[Vec<SchemaFrozenUnit>],
    latest_schemas: &[Vec<SchemaFrozenUnit>],
) -> VersionBump {
    let mut bump = VersionBump::None;

    // Map previous schemas by namespace for easy lookup
    let mut prev_map: HashMap<String, &Vec<SchemaFrozenUnit>> = HashMap::new();
    for schema in previous_schemas {
        if let Some(ns) = crate::schema::ir::frozen::unit::schema_namespace(schema) {
            prev_map.insert(ns.to_string(), schema);
        }
    }

    // Track visited namespaces to detect removals
    let mut visited_ns = HashSet::new();

    for latest in latest_schemas {
        if let Some(ns) = crate::schema::ir::frozen::unit::schema_namespace(latest) {
            visited_ns.insert(ns.to_string());
            match prev_map.get(ns) {
                Some(prev) => {
                    let schema_bump = compare_schema(prev, latest);
                    if schema_bump > bump {
                        bump = schema_bump;
                    }
                }
                None => {
                    // New schema added -> Minor
                    if VersionBump::Minor > bump {
                        bump = VersionBump::Minor;
                    }
                }
            }
        }
    }

    // Check for removals
    for ns in prev_map.keys() {
        if !visited_ns.contains(ns) {
            return VersionBump::Major; // Clean removal of a schema is breaking
        }
    }

    bump
}

fn compare_schema(prev: &[SchemaFrozenUnit], next: &[SchemaFrozenUnit]) -> VersionBump {
    let mut bump = VersionBump::None;

    // Build maps for items
    let mut prev_items = HashMap::new();
    for item in prev {
        if let Some(name) = get_item_name(item) {
            prev_items.insert(name, item);
        }
    }

    for item in next {
        if let Some(name) = get_item_name(item) {
            match prev_items.get(&name) {
                Some(prev_item) => {
                    let item_bump = compare_item(prev_item, item);
                    if item_bump > bump {
                        bump = item_bump;
                    }
                }
                None => {
                    // New item in existing schema -> Minor
                    if VersionBump::Minor > bump {
                        bump = VersionBump::Minor;
                    }
                }
            }
        }
    }

    // Check for removed items
    for (name, _) in prev_items {
        let found = next
            .iter()
            .any(|i| get_item_name(i).as_deref() == Some(name.as_str()));
        if !found {
            return VersionBump::Major;
        }
    }

    bump
}

fn get_item_name(item: &SchemaFrozenUnit) -> Option<String> {
    match item {
        SchemaFrozenUnit::Struct { name, .. } => Some(name.clone()),
        SchemaFrozenUnit::Enum { name, .. } => Some(name.clone()),
        SchemaFrozenUnit::Protocol { name, .. } => Some(name.clone()),
        SchemaFrozenUnit::Constant { name, .. } => Some(name.clone()),
        _ => None,
    }
}

fn compare_item(prev: &SchemaFrozenUnit, next: &SchemaFrozenUnit) -> VersionBump {
    match (prev, next) {
        (
            SchemaFrozenUnit::Struct {
                fields: prev_fields,
                ..
            },
            SchemaFrozenUnit::Struct {
                fields: next_fields,
                ..
            },
        ) => compare_struct_fields(prev_fields, next_fields),
        (
            SchemaFrozenUnit::Enum {
                variants: prev_variants,
                ..
            },
            SchemaFrozenUnit::Enum {
                variants: next_variants,
                ..
            },
        ) => compare_enum_variants(prev_variants, next_variants),
        (
            SchemaFrozenUnit::Protocol {
                functions: prev_funcs,
                ..
            },
            SchemaFrozenUnit::Protocol {
                functions: next_funcs,
                ..
            },
        ) => compare_protocol_functions(prev_funcs, next_funcs),
        // For constants or if type changed entirely
        _ => {
            if prev == next {
                VersionBump::None
            } else {
                VersionBump::Major
            }
        }
    }
}

fn compare_struct_fields(prev: &[SchemaFrozenUnit], next: &[SchemaFrozenUnit]) -> VersionBump {
    let mut bump = VersionBump::None;
    let mut prev_map = HashMap::new();

    for field in prev {
        if let SchemaFrozenUnit::Field { name, .. } = field {
            prev_map.insert(name, field);
        }
    }

    for field in next {
        if let SchemaFrozenUnit::Field { name, optional, .. } = field {
            match prev_map.get(name) {
                Some(prev_field) => {
                    // Field exists in both. If content changed -> Major.
                    // This includes type change, or optionality change.
                    // Relaxing optionality (Req -> Opt) is arguably Minor,
                    // but strictness says changing type signature is substantial.
                    // For now: any change to existing field is Major.
                    if prev_field != &field {
                        return VersionBump::Major;
                    }
                }
                None => {
                    // New field.
                    if *optional {
                        // Optional field added -> Minor
                        if VersionBump::Minor > bump {
                            bump = VersionBump::Minor;
                        }
                    } else {
                        // Required field added -> Major
                        return VersionBump::Major;
                    }
                }
            }
        }
    }

    // Check for removals
    for (name, _) in prev_items_map(prev) {
        let found = next.iter().any(|f| {
            if let SchemaFrozenUnit::Field { name: n, .. } = f {
                n == &name
            } else {
                false
            }
        });
        if !found {
            return VersionBump::Major;
        }
    }

    bump
}

fn compare_enum_variants(prev: &[SchemaFrozenUnit], next: &[SchemaFrozenUnit]) -> VersionBump {
    // Enum strictness: Any change is Major.
    if prev == next {
        VersionBump::None
    } else {
        VersionBump::Major
    }
}

fn compare_protocol_functions(prev: &[SchemaFrozenUnit], next: &[SchemaFrozenUnit]) -> VersionBump {
    // Protocol strictness: Any change is Major (for now).
    if prev == next {
        VersionBump::None
    } else {
        VersionBump::Major
    }
}

fn prev_items_map(items: &[SchemaFrozenUnit]) -> HashMap<String, &SchemaFrozenUnit> {
    let mut map = HashMap::new();
    for item in items {
        if let SchemaFrozenUnit::Field { name, .. } = item {
            map.insert(name.clone(), item);
        }
    }
    map
}
