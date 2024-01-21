// Standard Uses
use std::cell::Ref;

// Local Uses
use crate::schema::ir::context::SchemaContext;
use crate::schema::ir::compiler::report::CompileError;
use crate::schema::ir::compiler::report;
use crate::package::config::ir::context::ProjectContext;
use crate::report::ReportDetails;
use crate::utils::codemap::Span;

// External Uses
use strum_macros::EnumProperty;
use snafu::ResultExt;
use serde_derive::{Serialize, Deserialize};


#[derive(Debug, Eq, PartialEq, Clone)]
#[derive(Deserialize, Serialize, EnumProperty)]
#[repr(u8)]
pub enum Primitive {
    #[strum(props(Name="bool", Description="boolean, 1 byte"))]
    Boolean(Option<bool>) = 0,

    #[strum(props(Name="u8", Description="unsigned 1 byte, 8 bits"))]
    U8(Option<u8>),

    #[strum(props(Name="u16", Description="unsigned 2 bytes, 16 bits"))]
    U16(Option<u16>),

    #[strum(props(Name="u32", Description="unsigned 4 bytes, 32 bits"))]
    U32(Option<u32>),

    #[strum(props(Name="u64", Description="unsigned 8 bytes, 64 bits"))]
    U64(Option<u64>),

    #[strum(props(Name="u128", Description="unsigned 16 bytes, 128 bits"))]
    U128(Option<u128>),

    #[strum(props(Name="i8", Description=""))]
    S8(Option<i8>),

    #[strum(props(Name="i16", Description=""))]
    S16(Option<i16>),

    #[strum(props(Name="i32", Description=""))]
    S32(Option<i32>),

    #[strum(props(Name="i64", Description=""))]
    S64(Option<i64>),

    #[strum(props(Name="i128", Description=""))]
    S128(Option<i128>),

    // Float(f32), Double(f64),

    #[strum(props(Name="", Description=""))]
    String(Option<String>),

    #[strum(props(Name="", Description=""))]
    Namespaced(Option<String>)
}

impl Primitive {
    pub fn value_str(&self) -> Option<String> {
        match self {
            Primitive::Boolean(b) => { b.as_ref().map(|b| b.to_string()) }
            Primitive::U8(u) => { u.as_ref().map(|u| u.to_string()) }
            Primitive::U16(u) => { u.as_ref().map(|u| u.to_string()) }
            Primitive::U32(u) => { u.as_ref().map(|u| u.to_string()) }
            Primitive::U64(u) => { u.as_ref().map(|u| u.to_string()) }
            Primitive::U128(u) => { u.as_ref().map(|u| u.to_string()) }
            Primitive::S8(s) => { s.as_ref().map(|s| s.to_string()) }
            Primitive::S16(s) => { s.as_ref().map(|s| s.to_string()) }
            Primitive::S32(s) => { s.as_ref().map(|s| s.to_string()) }
            Primitive::S64(s) => { s.as_ref().map(|s| s.to_string()) }
            Primitive::S128(s) => { s.as_ref().map(|s| s.to_string()) }
            Primitive::String(s) => { s.as_ref().cloned() },
            Primitive::Namespaced(s) => {
                // TODO: This might be necessary to have more information
                //       kind of string instead of just clone
                s.as_ref().cloned()
            },
        }
    }

    pub fn name(&self) -> &str {
        use strum::EnumProperty;
        self.get_str("Name").unwrap()
    }

    pub fn name_description(&self) -> String {
        use strum::EnumProperty;
        format!("{}({})", self.get_str("Name").unwrap(), self.get_str("Description").unwrap())
    }

    /*
    fn kind_value_description(&self) -> (Discriminant<Primitive>, String) {
        let id = std::mem::discriminant(self);
        let value = self.value_str();

        (id.into(), value.unwrap())
    }
    */
}


#[derive(Debug, Eq, PartialEq, Clone)]
#[derive(Deserialize, Serialize)]
pub enum KindValue {
    Primitive(Primitive),
    EnumVariant(String, Option<Box<KindValue>>),
    Union(Vec<KindValue>),
    Namespaced(String, Option<Box<KindValue>>)
}

impl KindValue {
    pub fn name_and_value(&self) -> (String, Option<String>) {
        match self {
            KindValue::Primitive(primitive) => {
                return (primitive.name().to_owned(), primitive.value_str())
            }
            KindValue::EnumVariant(name, value) => {
                let value = value.as_ref().unwrap();
                let (variant_name, value) = value.name_and_value();

                (format!("Enum variant {} ({})'", name, variant_name), value)
            }
            KindValue::Union(_) => {
                todo!()
            }
            #[allow(unused)]
            KindValue::Namespaced(namespace, ..) => {
                // TODO: Properly implement, it was testing at this stage
                (namespace.clone(), None)
            }
        }
    }
}

pub fn resolve_kind_value(
    schema_context: &Ref<'_, SchemaContext>, project_context: &'_ ProjectContext,
    kind: &(Span, String), value: &Option<(Span, String)>
) -> Result<KindValue, Box<dyn snafu::Error>> {
    if value.is_none() {
        if let Some(kind) = to_kind_only(schema_context, project_context, kind) {
            return Ok(kind)
        }
    }

    if let Some(kind_value) = to_kind_value(
        schema_context, kind, value.as_ref().unwrap()
    ) {
        return Ok(kind_value)
    };

    Err(CompileError::TypeNotFound { name: kind.1.clone() })
        .context(report::CompileSnafu {
            details: ReportDetails::fetch(schema_context, &kind.0).unwrap()
        })?
}

#[allow(unused)]
pub(crate) fn to_kind_value(
    schema_context: &Ref<'_, SchemaContext>,
    kind: &(Span, String), value: &(Span, String)
) -> Option<KindValue> {
    if let Some(primitive) = to_primitive_kind_value(kind, &value.1) {
        return Some(KindValue::Primitive(primitive))
    }

    None
}

#[allow(unused)]
pub(crate) fn to_kind_only(
    schema_context: &Ref<'_, SchemaContext>, project_context: &'_ ProjectContext,
    kind: &(Span, String)
) -> Option<KindValue> {
    if let Some(primitive) = to_primitive_kind_only(kind) {
        return Some(KindValue::Primitive(primitive))
    }

    if let Some(namespaced) = to_namespaced_kind_only(schema_context, kind) {
        return Some(namespaced)
    }

    if let Some(union) = to_union_kind_only(schema_context, kind) {
        return Some(union)
    }

    None
}


fn to_namespaced_kind_only(
    schema_context: &Ref<'_, SchemaContext>, kind: &(Span, String)
) -> Option<KindValue> {
    let state = schema_context.compile_state.borrow();

    for (_, structure) in state.structures.iter() {
        if structure.name.1 == kind.1 {
            return Some(KindValue::Namespaced(
                structure.name.1.clone(), None
            ))
        }
    }

    for (_, constant) in state.consts.iter() {
        if constant.name.1 == kind.1 {
            return Some(KindValue::Namespaced(
                constant.name.1.clone(), None
            ))
        }
    }

    None
}


#[allow(unused)]
fn to_union_kind_only(
    schema_context: &Ref<'_, SchemaContext>, kind: &(Span, String)
) -> Option<KindValue> {
    todo!()
}

fn to_primitive_kind_only(kind: &(Span, String)) -> Option<Primitive> {
    use self::Primitive::*;
    match &*kind.1 {
        "u8" => Some(U8(None)),
        "u16" => Some(U16(None)),
        "u32" => Some(U32(None)),
        "u64" => Some(U64(None)),
        "u128" => Some(U128(None)),

        "s8" => Some(S8(None)),
        "s16" => Some(S16(None)),
        "s32" => Some(S32(None)),
        "s64" => Some(S64(None)),
        "s128" => Some(S128(None)),

        "str" => Some(String(None)),
        "bool" => Some(Boolean(None)),
        _ => { None }
    }
}

fn to_primitive_kind_value(
    kind: &(Span, String), value: &str
) -> Option<Primitive> {
    use self::Primitive::*;
    let kv = match &*kind.1 {
        "u8" => U8(Some(value.parse().unwrap())),
        "u16" => U16(Some(value.parse().unwrap())),
        "u32" => U32(Some(value.parse().unwrap())),
        "u64" => U64(Some(value.parse().unwrap())),
        "u128" => U128(Some(value.parse().unwrap())),

        "s8" => S8(Some(value.parse().unwrap())),
        "s16" => S16(Some(value.parse().unwrap())),
        "s32" => S32(Some(value.parse().unwrap())),
        "s64" => S64(Some(value.parse().unwrap())),
        "s128" => S128(Some(value.parse().unwrap())),

        "str" => String(Some(value.to_owned())),
        "bool" => Boolean(Some(value.parse().unwrap())),
        _ => { panic!("Got unknown type '{:#}'", kind.1) }
    };

    Some(kv)
}


