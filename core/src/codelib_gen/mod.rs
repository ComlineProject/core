// Standard Uses
use std::collections::HashMap;

// Crate Uses
use crate::schema::ir::frozen::unit::FrozenUnit;

// External Uses
use once_cell::sync::Lazy;


pub type VersionGenerators = Lazy<HashMap<&'static str, GeneratorFn>>;
pub type GeneratorFn = fn(&Vec<FrozenUnit>) -> String;
pub type Generator = (GeneratorFn, &'static str);


pub mod rust;

#[allow(unused)]
/// Find a generator function from the external codelib-gen library
pub fn find_generator(name: &str, _version: &str)
    -> Option<(&'static GeneratorFn, &'static str)>
{
    match name {
        "rust" => Some((&(rust::generate_rust as GeneratorFn), "rust")),
        _ => None,
    }
}

