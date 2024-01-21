// Standard Uses

// Local Uses

// External Uses

/*
/// Intermediate Representation Unit
#[allow(unused)]
#[derive(Debug, Eq, PartialEq)]
pub enum Unit {
    Items(Vec<Unit>),
    Tag(String),
    Namespace(String),
    Imports(String),
    Settings(Vec<Settings>),
    Consts(Vec<Const>),
    Structs(Vec<Struct>),
    Enums(Vec<Enum>),
    Errors(Vec<Error>),
    Protocols(Vec<Protocol>)
}


#[derive(Debug, Eq, PartialEq)]
pub struct Settings {
    pub id: String,
    pub parameters: Vec<Parameter>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Const {
    pub id: String,
    pub type_: Kind,
    pub default_value: Vec<u8>
}

#[allow(unused)]
#[derive(Debug, Eq, PartialEq)]
pub struct Struct {
    pub id: String,
    pub parameters: Vec<Parameter>,
    pub fields: Vec<Field>,
}

#[allow(unused)]
#[derive(Debug, Eq, PartialEq)]
pub struct Enum {
    pub id: String,
    pub variants: Vec<EnumVariant>
}

#[allow(unused)]
#[derive(Debug, Eq, PartialEq)]
pub struct EnumVariant {
    pub id: String,
    pub type_: Option<Kind>
}

#[allow(unused)]
#[derive(Debug, Eq, PartialEq)]
pub struct Error {
    pub id: String,
    pub parameters: Vec<Parameter>,
    pub fields: Vec<Field>,
}

#[allow(unused)]
#[derive(Debug, Eq, PartialEq)]
pub struct Parameter {
    pub id: String,
    pub value: Vec<u8>
}

#[allow(unused)]
#[derive(Debug, Eq, PartialEq)]
pub struct Field {
    pub index: u8,
    pub optional: bool,
    pub id: String,
    pub type_: Kind,
    pub default_value: Vec<u8>,
}

#[allow(unused)]
#[derive(Debug, Eq, PartialEq)]
pub struct Protocol {
    pub id: String,
    pub parameters: Vec<Parameter>,
    pub functions: Vec<Function>
}

#[allow(unused)]
#[derive(Debug, Eq, PartialEq)]
pub enum Direction { Client, Server, Both }

#[allow(unused)]
#[derive(Debug, Eq, PartialEq)]
pub struct Function {
    pub index: u8,
    pub id: String,
    pub async_: bool,
    pub direction: Direction,
    pub arguments: Vec<Argument>,
    pub return_: Vec<Kind>,
    pub parameters: Vec<Parameter>,
    pub throws: Vec<String>
}

#[allow(unused)]
#[derive(Debug, Eq, PartialEq)]
pub struct Argument {
    pub id: Option<String>,
    pub type_: Kind
}
*/