// Test beautiful diagnostics
use comline_core::schema::idl::diagnostics::print_parse_error;
use comline_core::schema::idl::grammar;

#[test]
fn test_beautiful_error() {
    let code = r#"
struct Test {
    name: string;
}
"#;
    
    let result = grammar::parse(code);
    
    if let Err(errors) = result {
        println!("\n=== BEAUTIFUL ERROR OUTPUT ===\n");
        for error in errors {
            print_parse_error(&error, code, "test.ids");
        }
        println!("\n=== END ERROR OUTPUT ===\n");
        
        // This test is meant to showcase the error output
        // Comment out the assertion to see the beautiful errors
        // assert!(false, "Check the error output above!");
    } else {
        panic!("Expected parse error but got success");
    }
}

#[test]
fn test_multiple_errors() {
    let code = r#"
struct HashMap {
    key: string;
    value: int;
}
"#;
    
    let result = grammar::parse(code);
    
    if let Err(errors) = result {
        println!("\n=== MULTIPLE ERRORS ===\n");
        for error in errors {
            print_parse_error(&error, code, "hashmap.ids");
        }
        println!("\n=== END ===\n");
    }
}
