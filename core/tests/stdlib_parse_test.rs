// Test: stdlib schemas parse correctly
use comline_core::schema::idl::grammar;

#[test]
fn test_stdlib_hashmap_parses() {
    let source = std::fs::read_to_string("stdlib/collections/HashMap.ids")
        .expect("Failed to read HashMap.ids");
    
    let result = grammar::parse(&source);
    if let Err(e) = &result {
        eprintln!("Parse error: {:?}", e);
    }
    assert!(result.is_ok(), "HashMap.ids should parse correctly");
    
    let doc = result.unwrap();
    assert!(!doc.0.is_empty(), "HashMap.ids should have declarations");
    
    println!("✅ HashMap.ids parsed successfully with {} declarations", doc.0.len());
}

#[test]
fn test_stdlib_vec_parses() {
    let source = std::fs::read_to_string("stdlib/collections/Vec.ids")
        .expect("Failed to read Vec.ids");
    
    let result = grammar::parse(&source);
    assert!(result.is_ok(), "Vec.ids should parse correctly");
    
    println!("✅ Vec.ids parsed successfully");
}

#[test]
fn test_stdlib_http_request_parses() {
    let source = std::fs::read_to_string("stdlib/http/Request.ids")
        .expect("Failed to read Request.ids");
    
    let result = grammar::parse(&source);
    assert!(result.is_ok(), "Request.ids should parse correctly");
    
    println!("✅ Request.ids parsed successfully");
}

#[test]
fn test_stdlib_http_response_parses() {
    let source = std::fs::read_to_string("stdlib/http/Response.ids")
        .expect("Failed to read Response.ids");
    
    let result = grammar::parse(&source);
    assert!(result.is_ok(), "Response.ids should parse correctly");
    
    println!("✅ Response.ids parsed successfully");
}
