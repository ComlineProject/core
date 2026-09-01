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
    //       Tag here means the same tag concept that CapNProto has
    // Tag(String),
    Namespace(String),
    Name(String),
    // Span is included in the hash/CAS identity deliberately: two schemas
    // that only differ in formatting/position should not be considered
    // content-identical.
    Import(String, (usize, usize)),
    Constant {
        docstring: Option<String>,
        name: String,
        kind_value: KindValue,
        span: (usize, usize),
    },
    Property {
        name: String,
        expression: Option<String>
    },
    Parameter {
        name: String,
        default_value: String
    },
    /// A validator's `validate { ... }` block: the parsed asserts.
    ExpressionBlock {
        asserts: Vec<FrozenUnit>
    },
    /// One `assert(condition, message)` from a `validate` block.
    Assert {
        /// Canonical condition text, e.g. `value.length >= params.min_chars`.
        condition: String,
        /// The message template, with `{path}` placeholders intact.
        message: String,
        /// Every `root.seg...` member path used in the condition (`value.*`,
        /// `params.*`, ...), for reference checking. Literals are not listed.
        references: Vec<String>,
    },
    //
    Enum {
        docstring: Option<String>,
        name: String,
        variants: Vec<FrozenUnit>,
        span: (usize, usize),
    },
    EnumVariant(KindValue, (usize, usize)),
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
        span: (usize, usize),
    },
    Protocol {
        docstring: String,
        parameters: Vec<FrozenUnit>,
        name: String,
        functions: Vec<FrozenUnit>,
        span: (usize, usize),
    },
    Function {
        docstring: String,
        // `@key = value` function annotations (per-call settings: `@timeout_ms`,
        // `@idempotent`, ...) as `Property { name, expression }`, same shape as
        // `Protocol` / `Struct` carry. Open namespace - a consumer acts on the
        // keys it knows and ignores the rest.
        parameters: Vec<FrozenUnit>,
        name: String,
        // direction: Box<FrozenUnit>,
        arguments: Vec<FrozenArgument>,
        _return: Option<KindValue>,
        // Names of the error(s) this function can throw - a reference by
        // name only, not resolved/validated against a declared `error`
        // (that's validator.rs-style work, out of scope here).
        throws: Vec<String>,
        span: (usize, usize),
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
    /// A validator applied to a field via `@validators = [Name(k = v, ...)]`.
    /// One per call in the list; `args` holds a `Property { name, expression }`
    /// per keyword argument.
    ValidatorRef {
        name: String,
        args: Vec<FrozenUnit>,
    },
    Field {
        docstring: Option<String>,
        parameters: Vec<FrozenUnit>,
        optional: bool,
        name: String,
        kind_value: KindValue,
        span: (usize, usize),
    }
}

#[derive(Deserialize, Serialize)]
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct  FrozenArgument {
    pub name: String,
    pub kind: KindValue,
    pub span: (usize, usize),
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

    // TODO: Since the variant FrozenUnit::Name was added, a split is not necessary anymore
    Some(namespace.split("::").collect::<Vec<_>>().join("/"))
}
