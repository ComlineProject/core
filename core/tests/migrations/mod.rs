// The format gate's tests that need a build (core#268, step 1.2): the fixture
// packages the migrations are proved on, what a fresh build of each hashes
// to, and the live types held to the frozen copy of the current format. The
// checks that look at the repository instead (the seal, the bump check) are
// `cargo xtask`'s.

mod corpus;
mod engine_current;
mod engine_formats;
mod golden;
mod live_frozen;
mod proof;
mod support;

// Step 1.3's engine (`cas::migrate`): `engine_current` on the format this
// comline writes, where the plan is empty and refusals, `verify`, `status`,
// `reset` and `prune` are what there is to test; `engine_formats` on
// test-only formats, which drive a real step through plan, journal,
// verification, swap, backup and resume after a crash at every point.
