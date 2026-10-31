# Comline core
Comline(Communication on Line) is a library/executable


## Language details
Made in Rust


## Resource Links
https://createlang.rs/

https://michael-f-bryan.github.io/static-analyser-in-rust/book/parse/parser.html

https://michael-f-bryan.github.io/static-analyser-in-rust/book/codemap.html

https://docs.rs/codemap/latest/codemap/


## Resources of ideas and inspirations
https://www.youtube.com/watch?v=ApHpmA1k73k


### Optimizing AST parsing and allocation
https://cs.hofstra.edu/~cscccl/rustlr_project/chapter6.html


## Building

`comline-core` compiles a tree-sitter grammar in its build script, so a C
compiler must be available on the build host.

### Features

- `compiler` (default): the parser, the `.ids`/`.idp` grammars, and
  everything built on them - compiling, validating, diffing, building and
  releasing packages.
- `deps` (implies `compiler`): dependency resolution, for the CLI.

With `default-features = false` the crate is the IR alone: the frozen schema
and config units, the CBOR helpers, the IR format stamp, the annotation names
and `unverified`. That's what a generator needs, and it has no tree-sitter in
it, so it builds for `wasm32-wasip1` and `wasm32-unknown-unknown`. Module
paths are the same with or without `compiler`.


## License

`comline-core` is licensed under the **GNU General Public License v3.0 only**
([LICENSE](LICENSE) or <https://www.gnu.org/licenses/gpl-3.0.html>).

It is part of Comline's *toolchain* — the compiler and code generators, which
you run to produce bindings. Copyleft keeps the toolchain itself open: a
distributed fork must be GPL. It does **not** reach your application — running
the generator no more licenses your program than compiling it with GCC does,
and generated code links only `comline-runtime`, which is MPL-2.0. See
[`design/licensing.md`](https://github.com/ComlineProject/docs) for the full
rationale and the per-repo split.

### Contribution

Unless you state otherwise, any contribution you submit for inclusion is
licensed GPL-3.0-only, without additional terms.

