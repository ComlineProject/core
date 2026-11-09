# Fuzzing the parsers and the compiler

[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) targets for what reads
user input: the `.ids` schema parser, the schema compiler behind it, and the
`config.idp` parser and interpreter. This crate is outside the workspace and
needs nightly.

```sh
core/fuzz/seed-corpus.sh
cargo +nightly fuzz run --fuzz-dir core/fuzz ids_parse -- -dict=core/fuzz/ids.dict -max_len=1024
```

## Targets

Each target takes any bytes, skips those that aren't UTF-8 (every reader
takes `&str`), and checks two things:

- **It never panics, overflows the stack or runs out of memory.**
- **A refusal is a diagnostic with a span:** the input gets at least one
  diagnostic back, and each one points inside it, on character boundaries,
  so the CLI and the language server can show where. `src/lib.rs` has the
  check.

| Target | What it runs |
|---|---|
| `ids_parse` | `schema::idl::parse::parse_schema`: the parser, the depth and integer-range checks, and the typed document. |
| `ids_compile` | `parse_schema`, then `IncrementalInterpreter::check_and_from_declarations` on what parses: aliases, bounds and settings checked, the frozen units built. |
| `idp_parse` | `package::config::idl::grammar::parse`, with the diagnostic `ProjectInterpreter::from_config_source` reports on a refusal, then `interpret_context` on what parses. |

`ids.dict` and `idp.dict` hold each grammar's tokens, so the fuzzer can write
keywords instead of finding them a byte at a time.

## Seeds

`seed-corpus.sh` copies the repo's own `.ids` and `.idp` files (std, the test
fixtures, the examples) into `corpus/<target>/`. The corpus isn't committed.

## CI

[`.github/workflows/fuzz.yml`](../../.github/workflows/fuzz.yml):

- **A pull request that touches the compiler:** each target runs for 60
  seconds, seeded from the repo.
- **Nightly:** each target runs for 10 minutes on a corpus cached between
  runs, with a process on every core (`-fork=N`). A failure uploads the
  crash and opens an issue titled after the target, unless one with that
  title is already open. It doesn't block anyone's pull request.

Every run writes `core.fuzz.json` (seconds, executions, corpus and findings
per target) with the Observatory's `report-fuzz` action, and a run on
`master` publishes it to the `reports` branch, where the
[Observatory](https://comlineproject.github.io/observatory/) charts it.

Inputs stop at 1 KiB (`-max_len=1024`). Under the address sanitizer the
parser costs about a millisecond per kilobyte, and what it gets wrong shows
up in short inputs.

## A finding

1. Reproduce it: `cargo +nightly fuzz run --fuzz-dir core/fuzz <target> artifacts/<target>/crash-…`.
2. Minimise it: `cargo +nightly fuzz tmin --fuzz-dir core/fuzz <target> artifacts/<target>/crash-…`.
3. Fix it, and add the minimised input as a regression test next to the code
   it broke, as a plain `#[test]`, so it runs on every platform without
   nightly.
