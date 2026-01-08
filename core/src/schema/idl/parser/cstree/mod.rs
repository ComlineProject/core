// External Imports
use cstree::{RawSyntaxKind, Syntax};


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
enum SyntaxKind {
    /* Tokens */
    Int,
    Plus,
    Minus,
    LParen,
    RParen,

    /* Nodes */
    Expr,
    Root,
}

type IDL = SyntaxKind;

impl Syntax for IDL {
    fn from_raw(raw: RawSyntaxKind) -> Self {
        todo!()
    }

    fn into_raw(self) -> RawSyntaxKind { RawSyntaxKind(self as u32) }

    fn static_text(self) -> Option<&'static str> {
        todo!()
    }
}

