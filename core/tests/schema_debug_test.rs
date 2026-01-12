// Test comparing working schema vs stdlib
use comline_core::schema::idl::grammar;

#[test]
fn test_compare_schemas() {
    // Test working schema
    let working = r#"
struct Test {
    value: u64;
}

protocol TestOps {
    function new() -> Test;
}
"#;
    
    let result1 = grammar::parse(working);
    println!("Working schema: {:?}", result1.is_ok());
    
    // Test HashMap-like schema
    let hashmap = r#"
struct HashMap {
    capacity: u64;
    size: u64;
}

protocol HashMapOps {
    function new() -> HashMap;
    function insert(key: string, value: string);
}
"#;
    
    let result2 = grammar::parse(hashmap);
    if let Err(e) = &result2 {
        println!("HashMap parse error: {:?}", e);
    }
    println!("HashMap schema: {:?}", result2.is_ok());
    
    assert!(result1.is_ok());
    assert!(result2.is_ok(), "HashMap schema should parse");
}
