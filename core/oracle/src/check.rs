//! The differential check: what core did with a graph against what the oracle
//! says it must.

use std::collections::BTreeSet;

use crate::model::{Model, G};
use crate::oracle::{predict, Prediction};
use crate::spec::Spec;

/// What building the root with core gave.
#[derive(Debug, Clone)]
pub enum Observed {
    /// It failed, with this error.
    Failed(String),
    /// It built; every protocol the build holds, with the guarantees frozen as
    /// given up (by name: `schema_agreement`, ...), empty for none.
    Built(Vec<(String, BTreeSet<String>)>),
}

/// Whether `observed` is what the oracle predicts for `m`'s root.
pub fn agrees(m: &Model, observed: &Observed) -> Result<(), String> {
    let prediction = predict(m, m.root());
    match (&prediction, observed) {
        (Prediction::Fails(_), Observed::Failed(_)) => Ok(()),
        (Prediction::Fails(reasons), Observed::Built(_)) => {
            Err(format!("core built it; the oracle says it can't, because:\n  {}", reasons.join("\n  ")))
        }
        (Prediction::Builds(_), Observed::Failed(error)) => Err(format!("core refused it; the oracle says it builds. core said:\n{error}")),
        (Prediction::Builds(expected), Observed::Built(found)) => {
            let mut problems = vec![];
            // Every protocol of the root's own package is in the build, and
            // each one the build holds that is one of the graph's gives up
            // what the oracle says.
            let root = m.root();
            for (name, set) in expected {
                let in_root = name.starts_with(&format!("P{root}S"));
                let seen: Vec<&BTreeSet<String>> = found.iter().filter(|(n, _)| n == name).map(|(_, s)| s).collect();
                if in_root && seen.is_empty() {
                    problems.push(format!("{name}: not in the build"));
                }
                let want: BTreeSet<String> = set.iter().map(|g: &G| g.name().to_string()).collect();
                for got in seen {
                    if *got != want {
                        problems.push(format!("{name}: core froze {got:?}, the oracle says {want:?}"));
                    }
                }
            }
            if problems.is_empty() {
                Ok(())
            } else {
                Err(problems.join("\n"))
            }
        }
    }
}

/// Build `spec`'s graph, and then the graph after its change, with `build`
/// (which writes the model's files and has core build the root), and check
/// each. The error says which, with the graph as files.
pub fn run(spec: &Spec, mut build: impl FnMut(&Model) -> Observed) -> Result<(), String> {
    let base = spec.model();
    let mut graphs = vec![("the graph", base.clone())];
    if let Some(change) = &spec.change {
        graphs.push(("after the upstream change", base.changed(change)));
    }
    for (what, m) in graphs {
        let observed = build(&m);
        agrees(&m, &observed).map_err(|why| format!("{what}: {why}\n\nthe packages (p{} is built):\n{}", m.root(), m.describe()))?;
    }
    Ok(())
}
