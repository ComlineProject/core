//! Wall-time benchmarks of the parser, by Criterion: `cargo bench -p
//! comline-core --bench parser_benchmark`. HTML reports land under
//! `target/criterion/`. CI runs it for the published report's `wall_ns` (not
//! gated: wall time on shared runners is noise); the instruction counts,
//! which are gated, are `parser_instructions.rs`, over the same inputs.
//!
//! Criterion's names map to the report's ids: `parse_simple_idl` is
//! `parse/simple`, `parse_complex_idl` is `parse/complex`, and
//! `parse_large_idl/<n>` is `parse/large/<n>`.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use comline_core::schema::idl::grammar;

mod inputs;

use inputs::*;

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
    for size in LARGE_SIZES {
        let idl = generate_large_idl(size);
        group.bench_with_input(BenchmarkId::from_parameter(size), &idl, |b, idl| {
            b.iter(|| grammar::parse(black_box(idl)))
        });
    }
    group.finish();
}

criterion_group!(benches, parse_simple, parse_complex, parse_large);
criterion_main!(benches);
