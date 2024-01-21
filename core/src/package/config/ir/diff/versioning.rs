// Standard Uses

// Crate Uses
use crate::package::config::ir::diff::Differ;
use crate::package::config::ir::frozen::{Dependency, FrozenUnit};

// External Uses


#[allow(unused)]
#[derive(Debug)]
pub struct Versioning {
    bump_major: bool,
    bump_minor: bool,
    bump_patch: bool
}
impl Versioning {
    pub fn new() -> Box<Self> {
        Box::new(Self { bump_major: false, bump_minor: false, bump_patch: false })
    }
}

impl Differ for Versioning {
    fn on_namespace_changed(&mut self, _: &str, _: &str) {
        self.bump_major = true;
    }

    #[allow(unused)]
    /// Changing the specification is considered a major change, because
    /// different specifications have different critical structure changes normally
    fn on_specification_changed(&mut self, old: u8, new: u8) {
        self.bump_major = true;
    }

    #[allow(unused)]
    fn on_schema_paths_changed(&mut self, old: &[String], new: &[String]) {
        self.bump_minor = true;
    }

    #[allow(unused)]
    fn on_dependencies_changed(&mut self, old: &[Dependency], new: &[Dependency]) {
        self.bump_minor = true;
    }
}

pub fn change() {
    // use semver::Version;
    // version: Version,
}


#[allow(unused)]
pub fn version_from(nodes: &[FrozenUnit]) -> Result<&String, ()> {
    /*
    for node in nodes {
        match node {
            _ => {}
        }
    }

    Err(())
    */

    todo!()
}
