
#[rust_sitter::grammar("idc")]
pub mod grammar {
    #[rust_sitter::language]
    #[derive(Debug, Clone)]
    pub struct Congregation {
        #[rust_sitter::leaf(text = "congregation")]
        _keyword: (),
        pub name: Identifier,
        pub assignments: Vec<Assignment>,
    }

    #[derive(Debug, Clone)]
    pub struct Assignment {
        pub key: Key,
        #[rust_sitter::leaf(text = "=")]
        _equals: (),
        pub value: Value,
    }

    #[derive(Debug, Clone)]
    pub enum Key {
        Identifier(Identifier),
        Namespaced(NamespacedKey),
        VersionMeta(ItemVersionMeta),
        DependencyAddress(DependencyAddress),
    }

    #[derive(Debug, Clone)]
    pub struct NamespacedKey {
        #[rust_sitter::leaf(pattern = r"[a-zA-Z0-9_]+(::[a-zA-Z0-9_]+)+", transform = |v| v.to_string())]
        pub value: String,
    }

    #[derive(Debug, Clone)]
    pub struct ItemVersionMeta {
        #[rust_sitter::leaf(pattern = r"[a-zA-Z0-9_]+(::[a-zA-Z0-9_]+)*#[a-zA-Z0-9_\.]+", transform = |v| v.to_string())]
        pub value: String,
    }

    #[derive(Debug, Clone)]
    pub struct DependencyAddress {
         #[rust_sitter::leaf(pattern = r"[a-zA-Z0-9_]+(::[a-zA-Z0-9_]+)*@[a-zA-Z0-9_\.]+(::[a-zA-Z0-9_\.]+)*", transform = |v| v.to_string())]
         pub value: String,
    }

    #[derive(Debug, Clone)]
    pub enum Value {
        String(StringLiteral),
        Number(NumberLiteral),
        Boolean(BooleanLiteral),
        List(List),
        Dictionary(Dictionary),
        Variable(Variable),
        Namespaced(NamespacedKey),
        Identifier(Identifier),
    }

    #[derive(Debug, Clone)]
    pub struct List {
        #[rust_sitter::leaf(text = "[")]
        _lbracket: (),
        #[rust_sitter::delimited(
            #[rust_sitter::leaf(text = ",")]
            ()
        )]
        pub items: Vec<Value>,
        #[rust_sitter::leaf(text = "]")]
        _rbracket: (),
    }

    #[derive(Debug, Clone)]
    pub struct Dictionary {
        #[rust_sitter::leaf(text = "{")]
        _lbrace: (),
        pub assignments: Vec<Assignment>,
        #[rust_sitter::leaf(text = "}")]
        _rbrace: (),
    }

    #[derive(Debug, Clone)]
    pub struct Identifier {
        #[rust_sitter::leaf(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*", transform = |v| v.to_string())]
        pub value: String,
    }

    #[derive(Debug, Clone)]
    pub struct StringLiteral {
        #[rust_sitter::leaf(pattern = r#""([^"\\]|\\["\\/bfnrt]|u[0-9a-fA-F]{4})*""#, transform = |v| v.to_string())]
        pub value: String,
    }

    #[derive(Debug, Clone)]
    pub struct NumberLiteral {
        #[rust_sitter::leaf(pattern = r"\d+", transform = |v| v.to_string())]
        pub value: String,
    }

    #[derive(Debug, Clone)]
    pub struct BooleanLiteral {
         #[rust_sitter::leaf(pattern = r"true|false", transform = |v| v.to_string())]
         pub value: String,
    }
    
    #[derive(Debug, Clone)]
    pub struct Variable {
        #[rust_sitter::leaf(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)+", transform = |v| v.to_string())]
        pub value: String,
    }

    #[rust_sitter::extra]
    pub struct Whitespace {
        #[rust_sitter::leaf(pattern = r"\s")]
        _whitespace: (),
    }
    
    #[rust_sitter::extra]
    pub struct Comment {
        #[rust_sitter::leaf(pattern = r"(//.*|/\*([^*]|\*[^/])*\*/)")]
        _comment: (),
    }
}

pub use grammar::*;
