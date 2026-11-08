//! Instruction-count benchmarks of the parser, by Callgrind through
//! `iai-callgrind`, over the same inputs as the wall-time bench
//! (`parser_benchmark.rs`).
//!
//! Run them with `cargo bench -p comline-core --bench parser_instructions`.
//! They need Valgrind and `iai-callgrind-runner` at the same version as the
//! `iai-callgrind` dev-dependency. CI fails a pull request when one grows by
//! more than 1%, unless it accepts the cost in `accepted-regressions.toml`
//! (see its header).
//!
//! Ids: the group, the function and the case, `parse/simple`,
//! `parse/complex`, `parse/large/<n>` (the case `n10` is `10`). Each input is
//! built by a setup function, which isn't measured, and handed back with the
//! result, so dropping it isn't measured either.

use std::hint::black_box;

use comline_core::schema::idl::grammar;
use iai_callgrind::{library_benchmark, library_benchmark_group, main};

mod inputs;

use inputs::*;

fn large_setup(size: usize) -> String {
    generate_large_idl(size)
}

#[library_benchmark]
fn simple() {
    let _ = black_box(grammar::parse(black_box(SIMPLE_IDL)));
}

#[library_benchmark]
fn complex() {
    let _ = black_box(grammar::parse(black_box(COMPLEX_IDL)));
}

// The sizes are `LARGE_SIZES`; the attribute can't take a constant's items.
#[library_benchmark]
#[bench::n10(args = (10), setup = large_setup)]
#[bench::n50(args = (50), setup = large_setup)]
#[bench::n100(args = (100), setup = large_setup)]
fn large(idl: String) -> String {
    let _ = black_box(grammar::parse(black_box(&idl)));
    idl
}

library_benchmark_group!(
    name = parse;
    benchmarks = simple, complex, large
);

main!(library_benchmark_groups = parse);
