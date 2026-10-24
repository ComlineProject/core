// Relative Modules
pub mod analyze;

pub use analyze::{analyze_config_changes, ConfigChange, ConfigChanges};

// Standard Uses
use std::fmt::Debug;

// Crate Uses
use crate::package::config::ir::frozen::{Dependency, FrozenUnit, FrozenWhole};

// External Uses
use downcast_rs::{Downcast, impl_downcast};


pub trait Differ: Downcast + Debug {
    fn on_namespace_changed(&mut self, old: &str, new: &str);
    fn on_specification_changed(&mut self, old: u8, new: u8);
    fn on_schema_paths_changed(&mut self, old: &[String], new: &[String]);
    fn on_dependencies_changed(&mut self, old: &[Dependency], new: &[Dependency]);

    // fn on_assignment_changed(&self, old: AssignmentUnit, new: AssignmentUnit);

    #[allow(unused)]
    fn differ(
        &self, previous: &[FrozenUnit], next: &FrozenWhole,
        document_gen: bool, auto_version: bool
    ) {
        // differ(previous, next, document_gen, auto_version)
    }
}
impl_downcast!(Differ);

#[allow(unused)]
pub fn match_differ(
    mut listeners: &mut Vec<Box<dyn Differ>>,
    previous: &[FrozenUnit], next: &[FrozenUnit]
) {
    for node in previous {
        use FrozenUnit::*;
        match node {
            Namespace(_) => {
                if let Some(l) = search_other(node, next) {
                    let Namespace(old) = node else { panic!() };
                    let Namespace(new) = l else { panic!() };

                    if old != new {
                        listeners.iter_mut().for_each(|mut l| l.on_namespace_changed(
                            old, new
                        ))
                    }
                }
            }
            SpecificationVersion(_) => {
                if let Some(l) = search_other(node, next) {
                    let SpecificationVersion(old) = node else { panic!() };
                    let SpecificationVersion(new) = l else { panic!() };

                    if old != new {
                        listeners.iter_mut().for_each(|mut l|
                            l.on_specification_changed(*old, *new)
                        )
                    }
                }
            }
            // The rest have no listener event yet.
            _ => {}
        }
    }
}


pub fn search_other<'a>(
    node: &FrozenUnit, others: &'a [FrozenUnit]
) -> Option<&'a FrozenUnit>
{
    for other in others {
        use FrozenUnit::*;
        match node {
            Namespace(_) => {
                if let Namespace(_) = other { return Some(other) };
            }
            SpecificationVersion(_) => {
                if let SpecificationVersion(_) = other { return Some(other) };
            }
            SchemaPath(_) => {
                if let SchemaPath(_) = other { return Some(other) };
            }
            Dependency(_) => {
                if let SchemaPath(_) = other { return Some(other) };
            }
            _ => {}
        }
    }

    None
}
