// Relative Modules
pub mod lang_items;
// pub mod validators;

// Standard Uses

// Local Uses
use crate::langlib::lang_items::LANGUAGE_ITEMS;
use crate::schema::idl::ast::unit::{ASTUnit, SpannedUnit};

// External Uses


pub fn find_unit<'a>(namespace: &str) -> Option<&'a Vec<SpannedUnit>> {
    let mut namespace_parts = namespace.split("::");

    for (_, units) in LANGUAGE_ITEMS.iter() {
        let unit_namespace = unit_namespace(units);
        if unit_namespace.is_none() { continue }
        let mut unit_namespace_parts = unit_namespace.unwrap().split("::");

        loop {
            let unit_part = unit_namespace_parts.next();
            let target_part = namespace_parts.next();

            if target_part.is_none() {
                return Some(units)
            }

            if unit_part.is_none() {
                let item = unit_item(
                    &units, target_part.unwrap()
                );

                if item.is_none() { continue }

                return Some(units)
            }

            if target_part != unit_part {
                return None
            }
        }
    }

    None
}

pub fn unit_namespace(unit: &Vec<SpannedUnit>) -> Option<&str> {
    for (_, variant) in unit {
        return match variant {
            ASTUnit::Namespace(_, n) => Some(n),
            _ => None
        }
    }

    None
}

pub fn unit_item<'a>(unit: &'a Vec<SpannedUnit>, unit_name: &str) -> Option<&'a SpannedUnit> {
    for spanned_unit in unit {
        match &spanned_unit.1 {
            ASTUnit::Constant { name: (_, name), .. }
            | ASTUnit::Enum { name: (_, name), .. }
            | ASTUnit::Settings { name: (_, name), .. }
            | ASTUnit::Struct { name: (_, name), .. }
            | ASTUnit::Error { name: (_, name), ..}
            | ASTUnit::Validator { name: (_, name), ..} => {
                if unit_name == name { return Some(spanned_unit) }
            },
            _ => continue
        }
    }

    None
}
