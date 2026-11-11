//! The differential check: what core did with a graph against what the oracle
//! says it must.

use std::collections::{BTreeMap, BTreeSet};

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

/// What a run covered: how many graphs there were and which way the oracle
/// decided them, so a test can tell it isn't vacuous (every graph refused, or
/// every one built with nothing given up).
#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub graphs: u32,
    /// Predicted to build, and built.
    pub built: u32,
    /// Predicted to fail, and refused; by the kinds of the reasons given
    /// (a graph can have several).
    pub refused: u32,
    pub refused_for: BTreeMap<&'static str, u32>,
    /// Graphs that built with more than one package.
    pub built_with_dependencies: u32,
    /// Protocols, among those that built, frozen as giving up each guarantee.
    pub frozen: BTreeMap<&'static str, u32>,
    /// Graphs whose upstream change turned a build into a refusal, or back.
    pub change_flipped: u32,
}

impl Stats {
    pub fn add(&mut self, other: &Stats) {
        self.graphs += other.graphs;
        self.built += other.built;
        self.refused += other.refused;
        self.built_with_dependencies += other.built_with_dependencies;
        self.change_flipped += other.change_flipped;
        for (k, v) in &other.refused_for {
            *self.refused_for.entry(k).or_default() += v;
        }
        for (k, v) in &other.frozen {
            *self.frozen.entry(k).or_default() += v;
        }
    }

    fn count(&mut self, m: &Model) {
        self.graphs += 1;
        match predict(m, m.root()) {
            Prediction::Fails(reasons) => {
                self.refused += 1;
                for kind in ["gate", "package-unverified", "bounds", "marker", "acceptance"] {
                    if reasons.iter().any(|r| r.starts_with(&format!("[{kind}]"))) {
                        *self.refused_for.entry(kind).or_default() += 1;
                    }
                }
            }
            Prediction::Builds(sets) => {
                self.built += 1;
                self.built_with_dependencies += (m.packages.len() > 1) as u32;
                for set in sets.values() {
                    for g in set {
                        *self.frozen.entry(g.name()).or_default() += 1;
                    }
                }
            }
        }
    }
}

/// What core's error says for each kind of reason the oracle gives: the codes
/// of its diagnostics, as the build renders them (`[unverified::missing-marker]`),
/// and the dependency check's text, which has none.
const KINDS: &[(&str, &[&str])] = &[
    ("gate", &["settings::forbidden"]),
    ("package-unverified", &["unverified::wrong-scope", "unverified::unknown-guarantee"]),
    ("bounds", &["bounds::unbounded", "unverified::missing-guarantee"]),
    ("marker", &["unverified::missing-marker", "unverified::missing-guarantee", "unverified::unknown-guarantee"]),
    ("acceptance", &["is unverified, giving up", "now gives up"]),
];

/// Whether `observed` is what the oracle predicts for `m`'s root.
pub fn agrees(m: &Model, observed: &Observed) -> Result<(), String> {
    let prediction = predict(m, m.root());
    match (&prediction, observed) {
        (Prediction::Fails(reasons), Observed::Failed(error)) => {
            // Refused, and for a reason the oracle has: an error that is
            // none of these is something else wrong with the graph.
            let says = |kind: &str| reasons.iter().any(|r| r.starts_with(&format!("[{kind}]")));
            let explained = KINDS.iter().any(|(kind, signs)| says(kind) && signs.iter().any(|sign| error.contains(sign)));
            if explained {
                Ok(())
            } else {
                Err(format!("core refused it, but not for what the oracle says:\n  {}\ncore said:\n{error}", reasons.join("\n  ")))
            }
        }
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
/// each. The error says which, with the graph as files; the `Stats` count
/// what the oracle decided.
pub fn run(spec: &Spec, mut build: impl FnMut(&Model) -> Observed) -> Result<Stats, String> {
    let mut stats = Stats::default();
    let base = spec.model();
    let mut graphs = vec![("the graph", base.clone())];
    if let Some(change) = &spec.change {
        graphs.push(("after the upstream change", base.changed(change)));
    }
    let mut decided = vec![];
    for (what, m) in graphs {
        stats.count(&m);
        decided.push(predict(&m, m.root()).is_builds());
        let observed = build(&m);
        agrees(&m, &observed).map_err(|why| format!("{what}: {why}\n\nthe packages (p{} is built):\n{}", m.root(), m.describe()))?;
    }
    stats.change_flipped += (decided.len() == 2 && decided[0] != decided[1]) as u32;
    Ok(stats)
}
