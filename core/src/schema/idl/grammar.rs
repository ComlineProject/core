// Comline IDL Grammar using rust-sitter
// This defines the grammar using Rust types and annotations

use rust_sitter::grammar;

#[grammar("idl")]
pub mod idl_grammar {
    use rust_sitter::{language, node};
    
    /// The IDL parser language definition
    #[language]
    pub struct IDL;
    
    /// Top-level document containing items
    #[node]
    pub struct Document {
        pub items: Vec<Item>,
    }
    
    /// An item in the IDL (struct, enum, interface, etc.)
    #[node]
    pub enum Item {
        StructDef(StructDef),
        EnumDef(EnumDef),
        InterfaceDef(InterfaceDef),
        Comment(Comment),
    }
    
    /// Structure definition
    #[node]
    pub struct StructDef {
        pub name: Identifier,
        pub fields: Vec<Field>,
    }
    
    /// Field in a struct
    #[node]
    pub struct Field {
        pub name: Identifier,
        pub type_: Type,
    }
    
    /// Enum definition  
    #[node]
    pub struct EnumDef {
        pub name: Identifier,
        pub variants: Vec<Variant>,
    }
    
    /// Enum variant
    #[node]
    pub struct Variant {
        pub name: Identifier,
        pub value: Option<i64>,
    }
    
    /// Interface definition (for RPC)
    #[node]
    pub struct InterfaceDef {
        pub name: Identifier,
        pub methods: Vec<Method>,
    }
    
    /// Method in an interface
    #[node]
    pub struct Method {
        pub name: Identifier,
        pub params: Vec<Field>,
        pub return_type: Option<Type>,
    }
    
    /// Type reference
    #[node]
    pub enum Type {
        Named(Identifier),
        Array { element: Box<Type> },
        Optional { inner: Box<Type> },
        Primitive(PrimitiveType),
    }
    
    /// Primitive types
    #[node]
    pub enum PrimitiveType {
        Int8, Int16, Int32, Int64,
        UInt8, UInt16, UInt32, UInt64,
        Float32, Float64,
        Bool,
        String,
        Bytes,
    }
    
    /// Identifier (names)
    #[node]
    pub struct Identifier {
        pub name: String,
    }
    
    /// Comment
    #[node]
    pub struct Comment {
        pub text: String,
    }
}

// Re-export for convenience
pub use idl_grammar::*;
