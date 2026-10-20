// Versioning tests - rewritten for CAS
// These tests verify that schema changes are correctly detected and version bumps applied

use comline_core::package::build::{build, release, ReleaseRequest, VersionBump};
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

/// Build, and release what was built: the 0.1.0 later builds compare against.
fn build_and_release(package_path: &PathBuf) {
    build(package_path).unwrap();
    release(package_path, &ReleaseRequest::default()).unwrap();
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
    assert_eq!(result1.current_version, "0.1.0-dev.1");
    
    // Second build with no changes
    let result2 = build(&package_path).unwrap();
    assert_eq!(result2.version_bump, VersionBump::None);
    assert_eq!(result2.current_version, "0.1.0-dev.1");
}

#[test]
fn test_struct_add_optional_field() {
    let (_temp, package_path) = setup_test_package();
    
    // Initial schema
    write_schema(&package_path, "ping", "struct Foo { a: s32 }");
    build_and_release(&package_path);
    
    // Add optional field
    write_schema(&package_path, "ping", "struct Foo { a: s32\n  optional b: string }");
    
    let result = build(&package_path).unwrap();
    assert_eq!(result.version_bump, VersionBump::Minor);
    assert_eq!(result.current_version, "0.1.1-dev.1");
}

#[test]
fn test_struct_add_required_field() {
    let (_temp, package_path) = setup_test_package();
    
    // Initial schema
    write_schema(&package_path, "ping", "struct Foo { a: s32 }");
    build_and_release(&package_path);
    
    // Add required field (breaking change)
    write_schema(&package_path, "ping", "struct Foo { a: s32\n  b: string }");
    
    let result = build(&package_path).unwrap();
    // Adding a required field is breaking: existing consumers that don't
    // supply it are no longer compatible.
    assert_eq!(result.version_bump, VersionBump::Major);
    assert_eq!(result.current_version, "0.2.0-dev.1");
}

#[test]
fn test_struct_field_default_value_change_is_a_patch() {
    let (_temp, package_path) = setup_test_package();

    // Initial schema
    write_schema(&package_path, "ping", "struct Foo { a: u32 = 1 }");
    build_and_release(&package_path);

    // Change only the default value - same field, same type, same
    // optionality. A default never changes what's on the wire, only what a
    // reader fills in, so it's a patch.
    write_schema(&package_path, "ping", "struct Foo { a: u32 = 2 }");

    let result = build(&package_path).unwrap();
    assert_eq!(result.version_bump, VersionBump::Patch);
    assert_eq!(result.current_version, "0.1.1-dev.1");
}

#[test]
fn test_struct_docstring_only_change_is_patch_bump() {
    let (_temp, package_path) = setup_test_package();

    // Initial schema
    write_schema(&package_path, "ping", "struct Foo { a: s32 }");
    build_and_release(&package_path);

    // Add a docstring - nothing else about the struct changes.
    write_schema(&package_path, "ping", "/// Now documented.\nstruct Foo { a: s32 }");

    let result = build(&package_path).unwrap();
    assert_eq!(result.version_bump, VersionBump::Patch);
    assert_eq!(result.current_version, "0.1.1-dev.1");
}

#[test]
fn test_added_error_is_minor_bump() {
    let (_temp, package_path) = setup_test_package();

    write_schema(&package_path, "ping", "struct Foo { a: s32 }");
    build_and_release(&package_path);

    write_schema(
        &package_path,
        "ping",
        "struct Foo { a: s32 }\n\nerror NotFoundError {\n    message = \"not found\"\n}",
    );

    let result = build(&package_path).unwrap();
    assert_eq!(result.version_bump, VersionBump::Minor);
    assert_eq!(result.current_version, "0.1.1-dev.1");
}

#[test]
fn test_removed_error_is_major_bump() {
    let (_temp, package_path) = setup_test_package();

    write_schema(
        &package_path,
        "ping",
        "struct Foo { a: s32 }\n\nerror NotFoundError {\n    message = \"not found\"\n}",
    );
    build_and_release(&package_path);

    write_schema(&package_path, "ping", "struct Foo { a: s32 }");

    let result = build(&package_path).unwrap();
    assert_eq!(result.version_bump, VersionBump::Major);
    assert_eq!(result.current_version, "0.2.0-dev.1");
}

#[test]
fn test_struct_remove_field() {
    let (_temp, package_path) = setup_test_package();
    
    // Initial schema with two fields
    write_schema(&package_path, "ping", "struct Foo { a: s32\n  b: string }");
    build_and_release(&package_path);
    
    // Remove field (breaking change)
    write_schema(&package_path, "ping", "struct Foo { a: s32 }");
    
    let result = build(&package_path).unwrap();
    assert_eq!(result.version_bump, VersionBump::Major);
    assert_eq!(result.current_version, "0.2.0-dev.1");
}

#[test]
fn test_new_schema() {
    let (_temp, package_path) = setup_test_package();
    
    // Initial schema
    write_schema(&package_path, "ping", "struct Foo { a: s32 }");
    build_and_release(&package_path);
    
    // Add new schema file
    write_schema(&package_path, "pong", "struct Bar { x: s32 }");
    
    // Update config - just rebuild since schemas auto-discovered from src/
    // No config change needed; the build will pick up the new schema file
    
    let result = build(&package_path).unwrap();
    assert_eq!(result.version_bump, VersionBump::Minor);
    assert_eq!(result.current_version, "0.1.1-dev.1");
}

#[test]
fn test_removed_schema() {
    let (_temp, package_path) = setup_test_package();
    
    // Initial with two schemas
    write_schema(&package_path, "ping", "struct Foo { a: s32 }");
    write_schema(&package_path, "pong", "struct Bar { x: s32 }");
    build_and_release(&package_path);
    
    // Remove pong schema file
    fs::remove_file(package_path.join("src/pong.ids")).unwrap();
    
    let result = build(&package_path).unwrap();
    assert_eq!(result.version_bump, VersionBump::Major);
    assert_eq!(result.current_version, "0.2.0-dev.1");
}

#[test]
fn test_new_schema_sorting_first_pairs_files_by_namespace() {
    let (_temp, package_path) = setup_test_package();

    write_schema(&package_path, "zeta", "struct Zeta { a: u32 }");
    build_and_release(&package_path);

    // `alpha` sorts before `zeta`. Paired by position, `zeta`'s old schema
    // would be compared with `alpha` and reported as removed: a major bump.
    write_schema(&package_path, "alpha", "struct Alpha { b: u32 }");

    let result = build(&package_path).unwrap();
    assert_eq!(result.version_bump, VersionBump::Minor);
    let changes = result.schema_changes.unwrap();
    assert!(changes.breaking_changes.is_empty(), "{changes:?}");
}
