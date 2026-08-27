// External Uses
use strum_macros::EnumProperty;
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

    #[strum(props(Name="s8", Description="signed 1 byte, 8 bits"))]
    S8(Option<i8>),

    #[strum(props(Name="s16", Description="signed 2 bytes, 16 bits"))]
    S16(Option<i16>),

    #[strum(props(Name="s32", Description="signed 4 bytes, 32 bits"))]
    S32(Option<i32>),

    #[strum(props(Name="s64", Description="signed 8 bytes, 64 bits"))]
    S64(Option<i64>),

    #[strum(props(Name="s128", Description="signed 16 bytes, 128 bits"))]
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
            KindValue::Union(members) => {
                let parts: Vec<String> = members
                    .iter()
                    .map(|member| member.name_and_value().0)
                    .collect();

                (format!("union({})", parts.join(" ")), None)
            }
            #[allow(unused)]
            KindValue::Namespaced(namespace, ..) => {
                // TODO: Properly implement, it was testing at this stage
                (namespace.clone(), None)
            }
        }
    }
}



