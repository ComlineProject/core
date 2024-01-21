// Standard Uses

// Crate Uses
use crate::schema::ir::diff::Differ;
use crate::schema::ir::frozen::unit::FrozenUnit;
use crate::schema::ir::compiler::interpreted::kind_search::KindValue;

// External Uses


#[allow(unused)]
#[derive(Debug)]
pub struct Document {
    pub major_changes: Vec<String>,
    pub minor_changes: Vec<String>,
}
#[allow(unused)]
impl Document {
    pub fn new() -> Box<Self> {
        Box::new(Self { major_changes: vec![], minor_changes: vec![] })
    }

    pub fn differ(&self, previous: &[FrozenUnit], next: &[FrozenUnit]) {
        todo!()
    }
}

#[allow(unused)]
impl Differ for Document {
    fn on_namespace_changed(&mut self, old: &str, new: &str) {
        self.major_changes.push(
            format!("Namespace changed from '{}' to '{}'", old, new)
        );
    }

    fn on_const_name_changed(&mut self, old: &str, new: &str) {
        self.minor_changes.push(
            format!("Constant name changed from '{}' to '{}'", old, new)
        );
    }

    fn on_const_kind_changed(&mut self, old: u8, new: u8) {
        todo!()
    }

    fn on_const_default_value_changed(
        &mut self, name: &str, left_kind_value: &KindValue, right_kind_value: &KindValue
    ) {
        let (left_kind_name, left_value_display) = {
            left_kind_value.name_and_value()
        };

        let (right_kind_name, right_value_display) = {
            right_kind_value.name_and_value()
        };

        if left_kind_name != right_kind_name {
            self.minor_changes.push(
                format!(
                    "Constant '{}' kind changed from '{}' to '{}'", name,
                    left_kind_name, right_kind_name
                )
            )
        }

        if left_value_display != right_value_display {
            self.minor_changes.push(
                format!(
                    "Constant '{}' default value changed from {} to {}", name,
                    into_formal(left_value_display),
                    into_formal(right_value_display)
                )
            )
        }
    }
}

fn into_formal(value: Option<String>) -> String {
    if let Some(value) = value { return format!("'{value}'") }

    "no default".to_owned()
}
