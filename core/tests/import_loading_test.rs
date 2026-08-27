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

    // Parse a real `use` statement and resolve it through the resolver end to end.
    let use_stmt_source = "use std::collections::HashMap";
    let document = grammar::parse(use_stmt_source).expect("use statement should parse");

    let comline_core::schema::idl::grammar::Declaration::Use(use_stmt) = &document.0[0].value else {
        panic!("Expected a Use declaration, got {:?}", document.0[0]);
    };

    let resolved = resolver
        .resolve(&use_stmt.path, &["mypackage".to_string()])
        .expect("stdlib use path should resolve");

    assert_eq!(
        resolved.absolute_namespace,
        vec!["std".to_string(), "collections".to_string(), "HashMap".to_string()]
    );
    assert_eq!(resolved.schema_path, Some(PathBuf::from("stdlib/collections/HashMap.ids")));

    let doc = resolver
        .load_schema(&resolved)
        .expect("resolved stdlib schema should load from disk");
    assert!(!doc.0.is_empty(), "HashMap schema should have declarations");
}
