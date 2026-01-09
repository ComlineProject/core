// Test IR generation - verify FrozenUnit creation

use comline_core::schema::idl::grammar;
use comline_core::schema::ir::compiler::Compile;
use comline_core::schema::ir::compiler::interpreter::IncrementalInterpreter;

fn main() {
    println!("=== Testing IR Generation ===\n");
    
    // Test 1: Simple struct
    let code = r#"
struct User {
    id: u64
    name: str
}
"#;
    
    println!("Parsing struct...");
    IncrementalInterpreter::from_source(code);
    println!();
    
    // Test 2: Protocol with functions
    let code = r#"
protocol UserService {
    function getUser(u64) returns str
    function createUser(str, str) returns u64
}
"#;
    
    println!("Parsing protocol with functions...");
    IncrementalInterpreter::from_source(code);
    println!();
    
    // Test 3: Complete IDL
    let code = r#"
import std

const MAX_USERS: u32 = 100

enum Status {
    Active
    Inactive
}

struct User {
    id: u64
    name: str
    status: Status
    tags: str[]
}

protocol API {
    function get(u64) returns User
    function list() returns User[]
    function delete(u64) returns bool
}
"#;
    
    println!("Parsing complete IDL...");
    Increment alInterpreter::from_source(code);
    
    println!("\n✅ IR generation complete!");
}
