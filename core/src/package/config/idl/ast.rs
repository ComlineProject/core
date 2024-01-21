// Standard Uses

// Local Uses
use crate::utils::codemap::{CodeMap, Span};

// External Uses


#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ASTUnit {
    Namespace(Span, String),
    Assignment {
        name: (Span, String),
        value: (Span, AssignmentUnit)
    },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AssignmentUnit {
    String(String),
    Reference(String),
    Number(u64),
    DependencyList(Vec<DependencyListItem>),
    List(Vec<ListItem>),
    Dictionary(Vec<DictKeyValue>),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DependencyListItem {
    name: (Span, String),
    author: (Span, String),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DictKeyValue {
    pub key: (Span, String),
    pub value: (Span, AssignmentUnit)
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ListItem {
    Number(Span, u64),
    String(Span, String),
    Path(Span, String),
}

pub type SpannedUnit = (Span, ASTUnit);
pub type SourcedWhole = (CodeMap, Vec<SpannedUnit>);

