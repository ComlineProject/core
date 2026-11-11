# Fuzzing the parsers and the compiler

[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) targets for what reads
user input: the `.ids` schema parser, the schema compiler behind it, and the
`config.idp` parser and interpreter, and the guarantee-safety oracle. This crate is outside the workspace and
needs nightly.

```sh
core/fuzz/seed-corpus.sh
cargo +nightly fuzz run --fuzz-dir core/fuzz ids_parse -- -dict=core/fuzz/ids.dict -max_len=1024
```

## Targets

The parsing targets take any bytes, skip those that aren't UTF-8 (every reader
takes `&str`), and check two things:

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
| `unverified_oracle` | A graph of packages drawn from the bytes, built with `compile_package` and checked against an oracle (below). |

### `unverified_oracle`

The one target whose input isn't text: the bytes are read as a package graph
(`core/oracle`'s `Spec`: one to three packages, their schemas, structs and
protocols, handshakes, `@unverified` markers, the package's own `unverified`,
each consumer's `accept_unverified`, and an upstream change between two
builds). It writes the graph as `config.idp` and `.ids` files, builds the root
the way `comline build` does, and checks the guarantee-safety invariant
(core#223): the build succeeds exactly when the oracle, working from the rules
alone, says every guarantee given up anywhere a protocol reaches was
acknowledged and accepted, and then each protocol's frozen given-up set is the
oracle's. It is the same check as the property test in
`tests/package/unverified_oracle.rs`; the graph, the oracle and the
`harness.rs` that builds with core are shared. A finding prints the graph as
its files. The oracle (`core/oracle`) has no dependency on `comline-core`; it
is its own small crate because the test and this target are in different
crates, and both need it without it needing core.

`ids.dict` and `idp.dict` hold each grammar's tokens, so the fuzzer can write
keywords instead of finding them a byte at a time.

## Seeds

`seed-corpus.sh` copies the repo's own `.ids` and `.idp` files (std, the test
fixtures, the examples) into `corpus/<target>/`. The corpus isn't committed.

## CI

[`.github/workflows/fuzz.yml`](../../.github/workflows/fuzz.yml):

- **A pull request that touches the compiler:** each target runs for 60
  seconds, seeded from the repo (`unverified_oracle` needs no seed or
  dictionary).
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
