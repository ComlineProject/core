// Criterion-based benchmark for rust-sitter parser

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
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

enum Status { Active, Inactive, Pending }
enum UserRole { Admin, User, Guest }

struct User {
    id: u64
    username: str
    email: str
    role: UserRole
    status: Status
    tags: str[]
}

protocol UserService {
    function getUser(u64) returns User
    function createUser(str, str, UserRole) returns u64
    function listUsers(u32, u32) returns User[]
    function deleteUser(u64) returns bool
}
"#;

fn generate_large_idl(num_structs: usize) -> String {
    let mut idl = String::from("import std\n\n");
    for i in 0..num_structs {
        idl.push_str(&format!(
            "struct Entity{} {{\n    id: u64\n    name: str\n    data: str[]\n}}\n\n",
            i
        ));
    }
    idl
}

fn parse_simple(c: &mut Criterion) {
    c.bench_function("parse_simple_idl", |b| {
        b.iter(|| grammar::parse(black_box(SIMPLE_IDL)))
    });
}

fn parse_complex(c: &mut Criterion) {
    c.bench_function("parse_complex_idl", |b| {
        b.iter(|| grammar::parse(black_box(COMPLEX_IDL)))
    });
}

fn parse_large(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_large_idl");
    
    for size in [10, 50, 100].iter() {
        let idl = generate_large_idl(*size);
        group.bench_with_input(BenchmarkId::from_parameter(size), &idl, |b, idl| {
            b.iter(|| grammar::parse(black_box(idl)))
        });
    }
    
    group.finish();
}

criterion_group!(benches, parse_simple, parse_complex, parse_large);

fn main() {
    benches();
    
    // Print report locations
    println!("\n🎯 Benchmark HTML Reports:");
    println!("📊 Main: target/criterion/report/index.html");
    println!("\nIndividual:");
    println!("  • Simple:  target/criterion/parse_simple_idl/report/index.html");
    println!("  • Complex: target/criterion/parse_complex_idl/report/index.html");
    println!("  • Large:   target/criterion/parse_large_idl/report/index.html");
    println!("\n💡 Open these files in your browser to view detailed charts\n");
}
