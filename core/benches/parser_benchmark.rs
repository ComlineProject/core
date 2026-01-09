// Performance benchmark for rust-sitter parser

use std::time::Instant;
use comline_core::schema::idl::grammar;

const SIMPLE_IDL: &str = r#"
struct User {
    id: u64
    name: str
}
"#;

const COMPLEX_IDL: &str = r#"
import std

const MAX_USERS: u32 = 1000
const API_VERSION: str = "v1.0"

enum Status {
    Active
    Inactive
    Pending
    Suspended
}

enum UserRole {
    Admin
    User
    Guest
    Moderator
}

struct Address {
    street: str
    city: str
    zip: str
    country: str
}

struct User {
    id: u64
    username: str
    email: str
    role: UserRole
    status: Status
    address: Address
    tags: str[]
    metadata: str[10]
}

struct UserList {
    users: User[]
    total: u32
    page: u32
    per_page: u32
}

protocol UserService {
    function getUser(u64) returns User
    function getUserByEmail(str) returns User
    function createUser(str, str, UserRole) returns u64
    function updateUser(u64, str, str) returns bool
    function deleteUser(u64) returns bool
    function listUsers(u32, u32) returns UserList
    function searchUsers(str) returns User[]
    function countUsers() returns u32
}

protocol AuthService {
    function login(str, str) returns str
    function logout(str) returns bool
    function validateToken(str) returns bool
    function refreshToken(str) returns str
}
"#;

fn benchmark_parse(name: &str, code: &str, iterations: usize) {
    let start = Instant::now();
    
    for _ in 0..iterations {
        let _ = grammar::parse(code);
    }
    
    let duration = start.elapsed();
    let avg_micros = duration.as_micros() / iterations as u128;
    
    println!("{}: {} iterations in {:?}", name, iterations, duration);
    println!("  Average: {}μs per parse", avg_micros);
    println!("  Throughput: {:.0} parses/second", 1_000_000.0 / avg_micros as f64);
}

fn main() {
    println!("=== Rust-Sitter Parser Performance Benchmark ===\n");
    
    // Warm up
    for _ in 0..100 {
        let _ = grammar::parse(SIMPLE_IDL);
    }
    
    println!("Simple IDL (4 lines):");
    benchmark_parse("Simple", SIMPLE_IDL, 10_000);
    println!();
    
    println!("Complex IDL (70+ lines):");
    benchmark_parse("Complex", COMPLEX_IDL, 1_000);
    println!();
    
    // Large file simulation
    let mut large_idl = String::from("import std\n\n");
    for i in 0..100 {
        large_idl.push_str(&format!(
            "struct Entity{} {{\n    id: u64\n    name: str\n    data: str[]\n}}\n\n",
            i
        ));
    }
    
    println!("Large IDL (~500 lines, 100 structs):");
    benchmark_parse("Large", &large_idl, 100);
    println!();
    
    println!("✅ Benchmark complete!");
}
