// Standard Uses

// Crate Uses
use crate::package::config::ir::diff::Differ;
use crate::package::config::ir::frozen::Dependency;

// External Uses


#[allow(unused)]
#[derive(Debug)]
pub struct Document {
    major_changes: Vec<String>,
    minor_changes: Vec<String>
}
impl Document {
    pub fn new() -> Box<Self> {
        Box::new(Self { major_changes: vec![], minor_changes: vec![] })
    }
}

impl Differ for Document {
    fn on_namespace_changed(&mut self, old: &str, new: &str) {
        self.major_changes.push(
            format!("Package namespace changed from {} to {}", old, new)
        );
    }

    fn on_specification_changed(&mut self, old: u8, new: u8) {
        self.major_changes.push(
            format!("Comline's specification version changed from {} to {}", old, new)
        )
    }

    #[allow(unused)]
    fn on_schema_paths_changed(&mut self, old: &[String], new: &[String]) {
        todo!()
    }

    #[allow(unused)]
    fn on_dependencies_changed(&mut self, old: &[Dependency], new: &[Dependency]) {
        todo!()
    }
}
