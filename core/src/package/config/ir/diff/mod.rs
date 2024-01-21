// Relative Modules
pub mod report;
pub mod versioning;

// Standard Uses
use std::fmt::Debug;

// Crate Uses
use crate::package::config::ir::frozen::{Dependency, FrozenUnit, FrozenWhole};
use crate::package::config::ir::diff::versioning::Versioning;
use crate::autodoc::document::Document;

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
pub fn differ(
    previous: &[FrozenUnit], next: &[FrozenUnit],
    document_gen: bool, auto_version: bool
) {
    /*
    let mut previous_version = versioning::version_from(previous).unwrap_or(
        &Version::parse("0.0.0").unwrap().to_string()
    );
    */

    let mut listeners: Vec<Box<dyn Differ>> = vec![];

    if document_gen { listeners.push(Document::new()) }
    if auto_version { listeners.push(Versioning::new()) }

    match_differ(&mut listeners, previous, next);

    let document = listeners[0].downcast_ref::<Document>().unwrap();
    let versioning = listeners[1].downcast_ref::<Versioning>().unwrap();
}

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
            SchemaPath(_) => {}
            Dependency(_) => {}
            missing => panic!("Node not implemented: {:?}", missing)
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
            missing => { panic!("Node not implemented: {:?}", missing) }
        }
    }

    None
}
