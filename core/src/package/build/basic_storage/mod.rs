// Relative Modules
// Relative Modules
pub mod package;

// Standard Uses
use std::fs::File;
use std::io::Write;
use std::path::Path;

// Crate Uses
use crate::package::build::basic_storage_project;
use crate::package::build::basic_storage_schema;
use crate::package::config::ir::context::ProjectContext;
use crate::schema::ir::diff::{analyze_schema_changes, SchemaChanges};

// External Uses
use eyre::Result;

/// Information returned from build processing
pub struct BuildInfo {
    pub version_bump: package::VersionBump,
    pub previous_version: Option<String>,
    pub current_version: String,
    pub schema_changes: Option<SchemaChanges>,
}

pub fn process_changes(project_path: &Path, latest_project: &ProjectContext) -> Result<BuildInfo> {
    let frozen_path = project_path.join(".frozen");

    check_frozen_data_integrity(project_path)?;
    let package_path = frozen_path.join("package/");

    let index_path = package_path.join("index");

    let versions_path = package_path.join("versions/");

    let previous_version = basic_storage_project::get_latest_version(&frozen_path).unwrap();
    let previous_version_path = versions_path.join(previous_version.to_string());

    let previous_project =
        basic_storage_project::deserialize::all_from_origin(&previous_version_path).unwrap();

    let previous_schemas =
        basic_storage_schema::deserialize::all_from_version_frozen(&previous_version_path)?;

    // Collect latest schemas for comparison
    let mut latest_schemas = vec![];
    for schema_ctx in &latest_project.schema_contexts {
        let schema_ref = schema_ctx.borrow();
        let frozen_ref = schema_ref.frozen_schema.borrow();
        if let Some(frozen) = frozen_ref.as_ref() {
            latest_schemas.push(frozen.clone());
        }
    }

    let bump = package::check_difference(&previous_schemas, &latest_schemas);
    
    // If no changes detected, return current version without bumping
    if bump == package::VersionBump::None {
        // No changes detected? Maybe only metadata? Or config changed?
        // For now, if no schema diff, we check project diff?
        // Or default to patch if we run this (implies user wants to publish).
        // Decision: Keep version unchanged - don't bump for no changes.
        tracing::debug!("No schema changes detected.");
        
        // Analyze schema changes (should be empty)
        let schema_changes = if !previous_schemas.is_empty() && !latest_schemas.is_empty() {
            let prev_flat: Vec<_> = previous_schemas
                .iter()
                .flat_map(|s| s.iter())
                .cloned()
                .collect();
            let latest_flat: Vec<_> = latest_schemas
                .iter()
                .flat_map(|s| s.iter())
                .cloned()
                .collect();
            Some(analyze_schema_changes(&prev_flat, &latest_flat))
        } else {
            None
        };
        
        // Return early - no need to freeze or bump version
        return Ok(BuildInfo {
            version_bump: bump,
            previous_version: Some(previous_version.to_string()),
            current_version: previous_version.to_string(), // Keep same version
            schema_changes,
        });
    }
    
    // Apply version bump
    let mut latest_version = previous_version.clone();
    match bump {
        package::VersionBump::Major => {
            latest_version.major += 1;
            latest_version.minor = 0;
            latest_version.patch = 0;
        }
        package::VersionBump::Minor => {
            latest_version.minor += 1;
            latest_version.patch = 0;
        }
        package::VersionBump::Patch => {
            latest_version.patch += 1;
        }
        package::VersionBump::None => unreachable!("Handled above"),
    }

    let latest_version_path = versions_path.join(latest_version.to_string());
    std::fs::create_dir_all(&latest_version_path)?;

    package::freeze_and_compare_packages(
        &previous_project,
        &previous_schemas,
        &latest_project,
        &latest_version_path,
    )?;

    File::options()
        .append(true)
        .open(&index_path)?
        .write(format!("\n{}", latest_version.to_string()).as_ref())?;

    // Analyze schema changes for detailed output
    let schema_changes = if !previous_schemas.is_empty() && !latest_schemas.is_empty() {
        // Flatten schemas for comparison
        let prev_flat: Vec<_> = previous_schemas
            .iter()
            .flat_map(|s| s.iter())
            .cloned()
            .collect();
        let latest_flat: Vec<_> = latest_schemas
            .iter()
            .flat_map(|s| s.iter())
            .cloned()
            .collect();
        Some(analyze_schema_changes(&prev_flat, &latest_flat))
    } else {
        None
    };

    Ok(BuildInfo {
        version_bump: bump,
        previous_version: Some(previous_version.to_string()),
        current_version: latest_version.to_string(),
        schema_changes,
    })
}

#[allow(unused)]
fn check_frozen_data_integrity(package_path: &Path) -> Result<()> {
    // TODO: Check integrity of the frozen contents, if they are valid,
    //       if something is broken, etc

    // todo!()
    Ok(())
}

#[allow(unused)]
pub(crate) fn process_initial_freezing(
    project_path: &Path,
    latest_project: &ProjectContext,
) -> Result<BuildInfo> {
    let frozen_path = project_path.join(".frozen/");

    /*
    let config_frozen = latest_project.config_frozen.as_ref().unwrap();
    let version = semver::Version::parse(
        frozen_project::version(config_frozen).unwrap()
    )?;

    if version.to_string() != MINIMUM_VERSION {
        bail!(
            "Initial version before freezing the package should be '{MINIMUM_VERSION}',\
             please change it in the package configuration"
        );
    }
    */

    package::freeze_project(&latest_project, &project_path)?;

    use crate::package::config::ir::frozen::MINIMUM_VERSION;

    Ok(BuildInfo {
        version_bump: package::VersionBump::None,
        previous_version: None,
        current_version: MINIMUM_VERSION.to_string(),
        schema_changes: None,
    })
}
