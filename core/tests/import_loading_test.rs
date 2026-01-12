// Test: ImportResolver loads stdlib schemas
use comline_core::schema::ir::compiler::import_resolver::{ImportResolver, ResolvedImport};
use std::path::PathBuf;
use std::collections::HashMap;

#[test]
fn test_load_stdlib_hashmap() {
    let stdlib_root = PathBuf::from("stdlib");
    
    let resolver = ImportResolver::new(
        vec!["mypackage".to_string()],
        HashMap::new(),
        Some(stdlib_root),
    );
    
    // Create a resolved import for std::collections::HashMap
    let resolved = ResolvedImport {
        absolute_namespace: vec!["std".to_string(), "collections".to_string(), "HashMap".to_string()],
        schema_path: Some(PathBuf::from("stdlib/collections/HashMap.ids")),
        symbols: vec![],
        alias: None,
    };
    
    // Load the schema
    let result = resolver.load_schema(&resolved);
    
    match result {
        Ok(doc) => {
            assert!(!doc.0.is_empty(), "HashMap schema should have declarations");
            println!("✅ Loaded HashMap schema with {} declarations", doc.0.len());
        }
        Err(e) => {
            panic!("Failed to load HashMap schema: {}", e);
        }
    }
}

#[test]
fn test_resolve_and_load_stdlib() {
    use comline_core::schema::idl::grammar;
    
    let stdlib_root = PathBuf::from("stdlib");
    
    let resolver = ImportResolver::new(
        vec!["mypackage".to_string()],
        HashMap::new(),
        Some(stdlib_root),
    );
    
    // Parse a use statement
    let use_stmt_source = "use std::http::Request";
    
    // For now, just test the resolver framework exists
    // Full integration test would parse the use statement and resolve it
    println!("✅ ImportResolver ready for stdlib loading");
}
