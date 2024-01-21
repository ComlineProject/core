// Standard Uses

// Crate Uses
use crate::schema::ir::context::SchemaContext;
use crate::schema::ir::compiler::interpreted::kind_search::KindValue;

// External Uses
use serde_derive::{Serialize, Deserialize};


pub type FrozenContextWhole = (SchemaContext, Vec<FrozenUnit>);


// TODO: A lot of string instances could be &'a str, boxes could also get ditched, etc
#[derive(Deserialize, Serialize)]
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum FrozenUnit {
    // TODO: Are Tags really necessary anymore since we hash Frozen Units by blob, trees and commits?
    // Tag(String),
    Namespace(String),
    Import(String),
    Constant {
        docstring: Option<String>,
        name: String,
        kind_value: KindValue
    },
    Property {
        name: String,
        expression: Option<String>
    },
    Parameter {
        name: String,
        default_value: String
    },
    ExpressionBlock {
        function_calls: Vec<String>
    },
    //
    Enum {
        docstring: Option<String>,
        name: String,
        variants: Vec<FrozenUnit>
    },
    EnumVariant(KindValue),
    Settings {
        docstring: Option<String>,
        name: String,
        parameters: Vec<FrozenUnit>,
    },
    Struct {
        docstring: Option<String>,
        parameters: Vec<FrozenUnit>,
        name: String,
        fields: Vec<FrozenUnit>,
    },
    Protocol {
        docstring: String,
        parameters: Vec<FrozenUnit>,
        name: String,
        functions: Vec<FrozenUnit>
    },
    Function {
        docstring: String,
        name: String,
        synchronous: bool,
        // direction: Box<FrozenUnit>,
        arguments: Vec<FrozenArgument>,
        _return: Option<KindValue>,
        throws: Vec<FrozenUnit>
    },
    Error {
        docstring: Option<String>,
        parameters: Vec<FrozenUnit>,
        name: String,
        message: String,
        fields: Vec<FrozenUnit>
    },
    Validator {
        docstring: Option<String>,
        properties: Vec<FrozenUnit>,
        name: String,
        expression_block: Box<FrozenUnit>
    },
    Field {
        docstring: Option<String>,
        parameters: Vec<FrozenUnit>,
        optional: bool,
        name: String,
        kind_value: KindValue,
    }
}

#[derive(Deserialize, Serialize)]
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct  FrozenArgument {
    pub name: String,
    pub kind: KindValue
}


pub fn schema_namespace(frozen: &[FrozenUnit]) -> Option<&str> {
    for unit in frozen {
        if let FrozenUnit::Namespace(name) = unit {
            return Some(name)
        }
    }

    None
}

pub fn schema_namespace_as_path(frozen: &[FrozenUnit]) -> Option<String> {
    let Some(namespace) = schema_namespace(frozen) else {
        return None
    };

    Some(namespace.split("::").collect::<Vec<_>>().join("/"))
}
