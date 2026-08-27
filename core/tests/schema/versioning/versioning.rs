// Versioning tests - rewritten for CAS
// These tests verify that schema changes are correctly detected and version bumps applied

use comline_core::package::build::{build, VersionBump};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

/// Create a minimal test package with config.idp and src/ directory
fn setup_test_package() -> (TempDir, PathBuf) {
    let temp_dir = tempfile::tempdir().unwrap();
    let package_path = temp_dir.path().to_path_buf();
    
    // Create config.idp with proper congregation DSL syntax
    let config = "congregation test\nspecification_version = 1\n";
    fs::write(package_path.join("config.idp"), config).unwrap();
    
    // Create src directory
    fs::create_dir(package_path.join("src")).unwrap();
    
    (temp_dir, package_path)
}

/// Write a simple schema file
fn write_schema(package_path: &PathBuf, name: &str, content: &str) {
    let schema_file = package_path.join(format!("src/{}.ids", name));
    fs::write(schema_file, content).unwrap();
}

#[test]
fn test_no_change() {
    let (_temp, package_path) = setup_test_package();
    
    // Initial schema
    write_schema(&package_path, "ping", "struct Foo { a: s32 }");
    
    // First build
    let result1 = build(&package_path).unwrap();
    assert_eq!(result1.current_version, "0.0.1");
    
    // Second build with no changes
    let result2 = build(&package_path).unwrap();
    assert_eq!(result2.version_bump, VersionBump::None);
    assert_eq!(result2.current_version, "0.0.1");
}

#[test]
fn test_struct_add_optional_field() {
    let (_temp, package_path) = setup_test_package();
    
    // Initial schema
    write_schema(&package_path, "ping", "struct Foo { a: s32 }");
    build(&package_path).unwrap();
    
    // Add optional field
    write_schema(&package_path, "ping", "struct Foo { a: s32\n  optional b: string }");
    
    let result = build(&package_path).unwrap();
    assert_eq!(result.version_bump, VersionBump::Minor);
    assert_eq!(result.current_version, "0.1.0");
}

#[test]
fn test_struct_add_required_field() {
    let (_temp, package_path) = setup_test_package();
    
    // Initial schema
    write_schema(&package_path, "ping", "struct Foo { a: s32 }");
    build(&package_path).unwrap();
    
    // Add required field (breaking change)
    write_schema(&package_path, "ping", "struct Foo { a: s32\n  b: string }");
    
    let result = build(&package_path).unwrap();
    // Adding a required field is breaking: existing consumers that don't
    // supply it are no longer compatible.
    assert_eq!(result.version_bump, VersionBump::Major);
    assert_eq!(result.current_version, "1.0.0");
}

#[test]
fn test_struct_field_default_value_change_is_not_breaking() {
    let (_temp, package_path) = setup_test_package();

    // Initial schema
    write_schema(&package_path, "ping", "struct Foo { a: u32 = 1 }");
    build(&package_path).unwrap();

    // Change only the default value - same field, same type, same
    // optionality. The diff system compares fields by type name only
    // (`types_compatible`/`kind_to_string` never look at the embedded
    // default), so this isn't detected as a change of any kind today -
    // this test locks in that it's at least not misdetected as a
    // *breaking* one. Finer-grained default-value diffing (a real Patch
    // bump for exactly this case) is a reasonable future enhancement,
    // not something this change attempts.
    write_schema(&package_path, "ping", "struct Foo { a: u32 = 2 }");

    let result = build(&package_path).unwrap();
    assert_eq!(result.version_bump, VersionBump::None);
    assert_eq!(result.current_version, "0.0.1");
}

#[test]
fn test_struct_docstring_only_change_is_patch_bump() {
    let (_temp, package_path) = setup_test_package();

    // Initial schema
    write_schema(&package_path, "ping", "struct Foo { a: s32 }");
    build(&package_path).unwrap();

    // Add a docstring - nothing else about the struct changes.
    write_schema(&package_path, "ping", "/// Now documented.\nstruct Foo { a: s32 }");

    let result = build(&package_path).unwrap();
    assert_eq!(result.version_bump, VersionBump::Patch);
    assert_eq!(result.current_version, "0.0.2");
}

#[test]
fn test_added_error_is_minor_bump() {
    let (_temp, package_path) = setup_test_package();

    write_schema(&package_path, "ping", "struct Foo { a: s32 }");
    build(&package_path).unwrap();

    write_schema(
        &package_path,
        "ping",
        "struct Foo { a: s32 }\n\nerror NotFoundError {\n    message = \"not found\"\n}",
    );

    let result = build(&package_path).unwrap();
    assert_eq!(result.version_bump, VersionBump::Minor);
    assert_eq!(result.current_version, "0.1.0");
}

#[test]
fn test_removed_error_is_major_bump() {
    let (_temp, package_path) = setup_test_package();

    write_schema(
        &package_path,
        "ping",
        "struct Foo { a: s32 }\n\nerror NotFoundError {\n    message = \"not found\"\n}",
    );
    build(&package_path).unwrap();

    write_schema(&package_path, "ping", "struct Foo { a: s32 }");

    let result = build(&package_path).unwrap();
    assert_eq!(result.version_bump, VersionBump::Major);
    assert_eq!(result.current_version, "1.0.0");
}

#[test]
fn test_struct_remove_field() {
    let (_temp, package_path) = setup_test_package();
    
    // Initial schema with two fields
    write_schema(&package_path, "ping", "struct Foo { a: s32\n  b: string }");
    build(&package_path).unwrap();
    
    // Remove field (breaking change)
    write_schema(&package_path, "ping", "struct Foo { a: s32 }");
    
    let result = build(&package_path).unwrap();
    assert_eq!(result.version_bump, VersionBump::Major);
    assert_eq!(result.current_version, "1.0.0");
}

#[test]
fn test_new_schema() {
    let (_temp, package_path) = setup_test_package();
    
    // Initial schema
    write_schema(&package_path, "ping", "struct Foo { a: s32 }");
    build(&package_path).unwrap();
    
    // Add new schema file
    write_schema(&package_path, "pong", "struct Bar { x: s32 }");
    
    // Update config - just rebuild since schemas auto-discovered from src/
    // No config change needed; the build will pick up the new schema file
    
    let result = build(&package_path).unwrap();
    assert_eq!(result.version_bump, VersionBump::Minor);
    assert_eq!(result.current_version, "0.1.0");
}

#[test]
fn test_removed_schema() {
    let (_temp, package_path) = setup_test_package();
    
    // Initial with two schemas
    write_schema(&package_path, "ping", "struct Foo { a: s32 }");
    write_schema(&package_path, "pong", "struct Bar { x: s32 }");
    build(&package_path).unwrap();
    
    // Remove pong schema file
    fs::remove_file(package_path.join("src/pong.ids")).unwrap();
    
    let result = build(&package_path).unwrap();
    assert_eq!(result.version_bump, VersionBump::Major);
    assert_eq!(result.current_version, "1.0.0");
}
