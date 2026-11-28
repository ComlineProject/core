// The proof harness on test-only formats (core#268, step 1.4; the design is
// `docs/proposals/ir-migrations/proof.md`). It proves the migration system
// before there is a second real format: three formats that exist only here
// (`formats`: T0 = 80, T1 = 81, T2 = 82, with real types and codecs over the
// real object store), the steps between them (`steps`), scripted histories
// that each format's "core" writes (`histories`), and the square those make
// (`square`): a store the core of N built, migrated by the engine, must equal
// what the core of the current format builds from the same history, after
// `forget` has put an unknown wherever the steps' own lists say N could not
// know.
//
//   squares    the honest steps on the corpus and on random histories; the
//              lossy step (an unknown with no room in the new type: a
//              placeholder, an entry that keeps the original) and the typed
//              unknown (`Known<T>`)
//   mistakes   steps wrong on purpose, one for each kind of mistake proof.md
//              names, and the check that rejects each
//   laws       totality, determinism (and committed digests every CI
//              platform compares), information preserved, idempotence,
//              composition, hashes carried, convergence
//   crashes    the engine killed at every `Point`, and again while resuming
//
// What the engine does with the lists is its own tests' business
// (`engine_formats`): this is about what a step must do for the lists to
// be true. What it does not prove is for the first real client (format 4):
// pins, and real fixtures built by the old core (`cargo xtask ir-fixtures`).

mod crashes;
mod formats;
mod histories;
mod laws;
mod mistakes;
mod square;
mod squares;
mod steps;
mod strategies;
