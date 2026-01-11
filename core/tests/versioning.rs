// DISABLED: These tests relied on basic_storage::package::check_difference()
// which has been removed along with basic_storage.
//
// TODO: Rewrite these tests to use CAS-based schema diffing:
// - Use crate::schema::ir::diff::analyze_schema_changes() for comparing schemas
// - Update test assertions to work with SchemaChanges instead of VersionBump  
// - Or move version bump logic into CAS and test that
//
// Original test cases that need to be rewritten:
// - test_no_change: No schema changes → VersionBump::None
// - test_struct_add_optional_field: Add optional field → VersionBump::Minor
// - test_struct_add_required_field: Add required field → VersionBump::Major  
// - test_struct_remove_field: Remove field → VersionBump::Major
// - test_new_schema: New schema added → VersionBump::Minor
// - test_removed_schema: Schema removed → VersionBump::Major

#[cfg(test)]
mod disabled_versioning_tests {
    // Tests temporarily disabled - need CAS rewrite
}
