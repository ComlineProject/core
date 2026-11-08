//! The parser benchmarks' inputs, shared by the wall-time bench
//! (`parser_benchmark.rs`, Criterion) and the instruction-count bench
//! (`parser_instructions.rs`, iai-callgrind), so both measure the same work.
//! Each is reported under one id: `parse/simple`, `parse/complex` and
//! `parse/large/<n>` (see .github/scripts/bench-report.py).

#![allow(dead_code)]

pub const SIMPLE_IDL: &str = r#"
struct User {
    id: u64
    name: string
}
"#;

pub const COMPLEX_IDL: &str = r#"
import std

const MAX_USERS: u32 = 1000
const API_VERSION: string = "v1.0"

enum Status { Active, Inactive, Pending }
enum UserRole { Admin, User, Guest }

struct User {
    id: u64
    username: string
    email: string
    role: UserRole
    status: Status
    tags: string[]
}

protocol UserService {
    function getUser(u64) returns User
    function createUser(string, string, UserRole) returns u64
    function listUsers(u32, u32) returns User[]
    function deleteUser(u64) returns bool
}
"#;

/// The sizes of `parse/large/<n>`: how many structs the schema has.
pub const LARGE_SIZES: [usize; 3] = [10, 50, 100];

pub fn generate_large_idl(num_structs: usize) -> String {
    let mut idl = String::from("import std\n\n");
    for i in 0..num_structs {
        idl.push_str(&format!(
            "struct Entity{} {{\n    id: u64\n    name: string\n    data: string[]\n}}\n\n",
            i
        ));
    }
    idl
}
