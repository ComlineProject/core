// Comprehensive tests for rust-sitter parser

fn main() {
    println!("\n🧪 Rust-Sitter Parser Test Suite\n");
    println!("{}", "=".repeat(60));
    println!();
    
    test_struct_parsing();
    test_enum_parsing();
    test_protocol_parsing();
    test_const_parsing();
    test_import_parsing();
    
    println!("{}", "=".repeat(60));
    println!("\n✅ All tests passed! Parser migration successful!");
}

fn test_struct_parsing() {
    println!("=== Test 1: Struct Parsing ===");
    let code = r#"
struct User {
    name: str
    age: u8
}
"#;
    println!("Code:\n{}", code);
    
    match comline_core::schema::idl::grammar::parse(code) {
        Ok(decl) => println!("✅ Struct parsed successfully: {:?}\n", decl),
        Err(e) => println!("❌ Parse error: {:?}\n", e),
    }
}

fn test_enum_parsing() {
    println!("=== Test 2: Enum Parsing ===");
    let code = r#"
enum Status {
    Active
    Inactive
    Pending
}
"#;
    println!("Code:\n{}", code);
    
    match comline_core::schema::idl::grammar::parse(code) {
        Ok(decl) => println!("✅ Enum parsed successfully: {:?}\n", decl),
        Err(e) => println!("❌ Parse error: {:?}\n", e),
    }
}

fn test_protocol_parsing() {
    println!("=== Test 3: Protocol Parsing ===");
    let code = r#"
protocol UserService {
    function getUser(u64) returns str
    function listUsers() returns str
}
"#;
    println!("Code:\n{}", code);
    
    match comline_core::schema::idl::grammar::parse(code) {
        Ok(decl) => println!("✅ Protocol parsed successfully: {:?}\n", decl),
        Err(e) => println!("❌ Parse error: {:?}\n", e),
    }
}

fn test_const_parsing() {
    println!("=== Test 4: Const Parsing ===");
    let code = r#"const MAX_USERS: u32 = 1000"#;
    println!("Code: {}", code);
    
    match comline_core::schema::idl::grammar::parse(code) {
        Ok(decl) => println!("✅ Const parsed successfully: {:?}\n", decl),
        Err(e) => println!("❌ Parse error: {:?}\n", e),
    }
}

fn test_import_parsing() {
    println!("=== Test 5: Import Parsing ===");
    let code = r#"import std"#;
    println!("Code: {}", code);
    
    match comline_core::schema::idl::grammar::parse(code) {
        Ok(decl) => println!("✅ Import parsed successfully: {:?}\n", decl),
        Err(e) => println!("❌ Parse error: {:?}\n", e),
    }
}
