// Example usage of the new BuildResult API

use comline_core::package::build::{build, BuildResult, VersionBump};
use comline_core::schema::ir::diff::{BreakingChange, NewFeature, SchemaChanges};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let package_path = Path::new(".");
    
    // Build the package
    let result = build(package_path)?;
    
    // Display version information
    if let Some(version_change) = result.version_change() {
        println!("📦 Version changed: {}", version_change);
    } else if result.is_initial_build() {
        println!("📦 Initial build: {}", result.current_version);
    } else {
        println!("📦 Version: {} (no change)", result.current_version);
    }
    
    // Display version bump type
    match result.version_bump {
        VersionBump::Major => println!("🔴 MAJOR version bump (breaking changes)"),
        VersionBump::Minor => println!("🟢 MINOR version bump (new features)"),
        VersionBump::Patch => println!("🔵 PATCH version bump (bug fixes)"),
        VersionBump::None => println!("⚪ No version bump needed"),
    }
    
    // Display schema changes if available
    if let Some(changes) = &result.schema_changes {
        display_schema_changes(changes);
    } else {
        println!("ℹ️  No previous version to compare");
    }
    
    println!("✅ Build successful!");
    
    Ok(())
}

fn display_schema_changes(changes: &SchemaChanges) {
    if changes.is_empty() {
        println!("ℹ️  No schema changes detected");
        return;
    }
    
    // Breaking changes
    if !changes.breaking_changes.is_empty() {
        println!("\n🔴 Breaking Changes ({}):", changes.breaking_changes.len());
        for change in &changes.breaking_changes {
            match change {
                BreakingChange::RemovedStruct { name } => {
                    println!("  - Removed struct `{}`", name);
                }
                BreakingChange::RemovedField { type_name, field_name } => {
                    println!("  - Removed field `{}` from `{}`", field_name, type_name);
                }
                BreakingChange::ChangedFieldType { type_name, field_name, old_type, new_type } => {
                    println!("  - Changed `{}.{}`: {} → {}", type_name, field_name, old_type, new_type);
                }
                BreakingChange::RemovedFunction { protocol_name, function_name } => {
                    println!("  - Removed function `{}::{}()`", protocol_name, function_name);
                }
                BreakingChange::ChangedFunctionSignature { protocol_name, function_name, details } => {
                    println!("  - Changed `{}::{}()`: {}", protocol_name, function_name, details);
                }
                BreakingChange::RemoveEnum { name } => {
                    println!("  - Removed enum `{}`", name);
                }
                BreakingChange::RemovedEnumVariant { enum_name, variant } => {
                    println!("  - Removed variant `{}::{}` BreakingChange::RemovedProtocol { name } => {
                    println!("  - Removed protocol `{}`", name);
                }
            }
        }
    }
    
    // New features
    if !changes.new_features.is_empty() {
        println!("\n🟢 New Features ({}):", changes.new_features.len());
        for feature in &changes.new_features {
            match feature {
                NewFeature::AddedStruct { name, field_count } => {
                    println!("  + Added struct `{}` ({} fields)", name, field_count);
                }
                NewFeature::AddedField { type_name, field_name, field_type, optional } => {
                    let opt_marker = if *optional { " (optional)" } else { "" };
                    println!("  + Added field `{}.{}`: {}{}", type_name, field_name, field_type, opt_marker);
                }
                NewFeature::AddedFunction { protocol_name, function_name, signature } => {
                    println!("  + Added function `{}::{}`: {}", protocol_name, function_name, signature);
                }
                NewFeature::AddedEnum { name, variant_count } => {
                    println!("  + Added enum `{}` ({} variants)", name, variant_count);
                }
                NewFeature::AddedEnumVariant { enum_name, variant } => {
                    println!("  + Added variant `{}::{}`", enum_name, variant);
                }
                NewFeature::AddedProtocol { name, function_count } => {
                    println!("  + Added protocol `{}` ({} functions)", name, function_count);
                }
            }
        }
    }
    
    // Modifications
    if !changes.modifications.is_empty() {
        println!("\n🔵 Modifications ({}):", changes.modifications.len());
        for modification in &changes.modifications {
            match modification {
                crate::schema::ir::diff::Modification::FieldMadeOptional { type_name, field_name } => {
                    println!("  ~ Field `{}.{}` marked as optional", type_name, field_name);
                }
            }
        }
    }
}
