// Standard Uses
use std::fmt::Debug;

// Crate Uses
use crate::schema::ir::frozen::unit::{FrozenUnit, FrozenContextWhole};
use crate::schema::ir::compiler::interpreted::kind_search::KindValue;

// External Uses
use downcast_rs::{Downcast, impl_downcast};


#[allow(unused)]
pub trait Differ: Downcast + Debug {
    fn on_namespace_changed(&mut self, old: &str, new: &str);
    fn on_const_name_changed(&mut self, old: &str, new: &str);
    fn on_const_kind_changed(&mut self, old: u8, new: u8);
    fn on_const_default_value_changed(
        &mut self, name: &str, left_kind_value: &KindValue, right_kind_value: &KindValue
    );

    fn differ(
        &self, previous: &[FrozenUnit], next: &FrozenContextWhole,
        document_gen: bool, auto_version: bool
    ) {
        todo!()
    }
}
impl_downcast!(Differ);


#[allow(unused)]
pub fn match_differ(
    mut listeners: &mut Vec<Box<dyn Differ>>,
    previous: &Vec<FrozenUnit>, next: &Vec<FrozenUnit>
) {
    for left in previous {
        match left {
            FrozenUnit::Namespace(..) => {
                if let Some(r) = search_other(left, next) {
                    let FrozenUnit::Namespace(left) = left else { panic!() };
                    let FrozenUnit::Namespace(right) = r else { panic!() };

                    if left != right {
                        listeners.iter_mut().for_each(|mut l| l.on_namespace_changed(
                            left, right
                        ))
                    }
                }
            }
            FrozenUnit::Import(..) => {
                todo!()
            }
            FrozenUnit::Constant {
                docstring: l_docstring, name: l_name, kind_value: l_kind_value
            } => {
                if let Some(r) = search_other(left, next) {
                    let FrozenUnit::Constant {
                        docstring, name, kind_value
                    } = r else { panic!() };

                    if l_name != name {
                        listeners.iter_mut().for_each(|mut differ| {
                            differ.on_const_name_changed(l_name, name)
                        });
                    }

                    if l_kind_value != kind_value {
                        listeners.iter_mut().for_each(|mut differ| {
                           differ.on_const_default_value_changed(
                               name,l_kind_value, kind_value
                           )
                        });
                    }
                }
            }
            FrozenUnit::Struct{..} => {
                if let Some(l) = search_other(left, next) {
                    let FrozenUnit::Struct{..} = left else { panic!() };
                    let FrozenUnit::Struct{..} = l else { panic!() };

                    if left != l {
                        /*
                        listeners.iter_mut().for_each(|mut l|
                            l.on_structure_changed(node, l)
                        )
                        */
                    }
                }
            }
            missing => panic!("Node not implemented: {:?}", missing)
        }
    }
}


pub fn search_other<'a>(node: &FrozenUnit, others: &'a Vec<FrozenUnit>)
    -> Option<&'a FrozenUnit>
{
    for other in others {
        match node {
            FrozenUnit::Namespace(..) => {
                if let FrozenUnit::Namespace(..) = other { return Some(other) };
            }
            FrozenUnit::Import(..) => {
                if let FrozenUnit::Import(..) = other { return Some(other) };
            }
            FrozenUnit::Constant{..} => {
                if let FrozenUnit::Constant{..} = other { return Some(other) };
            }
            FrozenUnit::Struct{..} => {
                if let FrozenUnit::Struct{..} = other { return Some(other) };
            }
            missing => { panic!("Node not implemented: {:?}", missing) }
        }
    }

    None
}
