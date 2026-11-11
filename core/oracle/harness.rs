//! Building a graph with core: the side of the check that depends on
//! `comline-core`, so it isn't in the oracle crate (which mustn't). Included
//! by `tests/package/unverified_oracle.rs` and by `fuzz/fuzz_targets/
//! unverified_oracle.rs` with `#[path]`, since a crate both could depend on
//! would depend on core, and core's tests on it: one copy keeps what the
//! test and the fuzz target observe the same.
//!
//! Needs `comline-core` with `deps`, and `tempfile`.

use std::collections::BTreeSet;

use comline_core::package::build::compile_package;
use comline_core::schema::ir::frozen::unit::FrozenUnit;
use comline_core::schema::ir::unverified;
use comline_core_oracle::check::Observed;
use comline_core_oracle::model::Model;

/// Write the graph's packages side by side in a temporary directory, build
/// the root as `comline build` does (`compile_package`, dependencies from
/// the paths its config declares), and read what it gave: whether it built,
/// and every protocol in the build with the set frozen as given up.
pub fn build(m: &Model) -> Observed {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    for (path, text) in m.files() {
        let path = tmp.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).expect("a package directory");
        std::fs::write(path, text).expect("a package file");
    }
    let context = match compile_package(&tmp.path().join(format!("p{}", m.root()))) {
        Ok(context) => context,
        Err(error) => return Observed::Failed(format!("{error:?}")),
    };
    let mut protocols = vec![];
    for schema in &context.schema_contexts {
        let schema = schema.borrow();
        let frozen = schema.frozen_schema.borrow();
        for unit in frozen.as_deref().unwrap_or(&[]) {
            if let FrozenUnit::Protocol { name, parameters, .. } = unit {
                let set: BTreeSet<String> = unverified::frozen_set(parameters).iter().map(|g| g.name().to_string()).collect();
                protocols.push((name.clone(), set));
            }
        }
    }
    Observed::Built(protocols)
}
