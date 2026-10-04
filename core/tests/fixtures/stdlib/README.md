# Parser fixtures (not the standard library)

Old placeholder schemas, kept only as inputs for `stdlib_parse_test.rs` and
`import_loading_test.rs` (the resolver's on-disk `stdlib_root` loading).

The real standard library is the `std` package in
[`core_stdlib/packages/std`](../../../../core_stdlib/packages/std), embedded
into `comline-core` and available to every package as `use std::…`.
