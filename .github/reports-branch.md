# Published reports

This branch is written by CI only; don't push to it by hand. It holds the
latest reports of `master`, which the Observatory reads:

- `core.bench.json`: the parser benches, from `.github/workflows/bench.yml`
  on every push to `master`. Bench format 2: per benchmark id
  (`parse/simple`, `parse/complex`, `parse/large/<n>`), `instructions`
  (iai-callgrind, `core/benches/parser_instructions.rs`) and `wall_ns`
  (Criterion's mean and its spread, `core/benches/parser_benchmark.rs`).
  Written by `.github/scripts/bench-report.py` on `master`.
- `core.coverage.json`, `core.compile.json`: line/function coverage per crate
  and a clean release build's seconds per crate, from
  `.github/workflows/reports.yml` on every push to `master`. Formats: the
  Observatory's `DATA.md` (`coverage`, `compile`).
- `core.fuzz.json`: seconds, executions, corpus and findings per fuzz target
  (`ids_parse`, `ids_compile`, `idp_parse`), from `.github/workflows/fuzz.yml`
  nightly. Format: the Observatory's `DATA.md` (`fuzz`), written by its
  `report-fuzz` action.

Each report names the `master` commit it measured. Every publish is one
commit here, so `git log -- core.bench.json` is the series. Publishing is the
Observatory's `publish-report` action.
