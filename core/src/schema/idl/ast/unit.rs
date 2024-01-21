// Standard Uses
use std::rc::Rc;
use std::cell::RefCell;

// Local Uses
use crate::utils::codemap::{CodeMap, Span};

// External Uses
use serde_derive::{Serialize, Deserialize};


pub type OrderIndex = u16;

#[derive(Debug, Eq, PartialEq)]
#[derive(Serialize, Deserialize)]
pub enum Direction { Client, Server, Both }


#[derive(Debug, Eq, PartialEq, Hash)]
#[derive(Serialize, Deserialize)]
pub enum ASTUnit {
    Namespace(Span, String),
    Import(Span, String),
    Docstring {
        variable: Option<String>,
        description: String
    },
    Constant {
        docstring: Vec<ASTUnit>,
        name: (Span, String),
        kind: (Span, String),
        default_value: Option<(Span, String)>,
    },
    Property {
        name: (Span, String),
        expression: Option<(Span, String)>
    },
    Parameter {
        name: (Span, String),
        default_value: (Span, String)
    },
    ExpressionBlock {
        function_calls: Vec<String>
    },
    //
    Enum {
        docstring: Vec<ASTUnit>,
        name: (Span, String),
        variants: Vec<SpannedUnit>
    },
    EnumVariant {
        name: (Span, String),
        kind: Option<(Span, String)>
    },
    Settings {
        docstring: Vec<SpannedUnit>,
        name: (Span, String),
        parameters: Vec<SpannedUnit>,
    },
    Struct {
        docstring: Vec<SpannedUnit>,
        parameters: Vec<SpannedUnit>,
        name: (Span, String),
        fields: Vec<SpannedUnit>,
    },
    Protocol {
        docstring: Vec<SpannedUnit>,
        parameters: Vec<SpannedUnit>,
        name: (Span, String),
        functions: Vec<SpannedUnit>
    },
    Function {
        docstring: Vec<SpannedUnit>,
        parameters: Vec<SpannedUnit>,
        name: (Span, String),
        asynchronous: Option<Span>,
        // direction: Direction,
        arguments: Vec<SpannedUnit>,
        // returns: Vec<ASTUnit>,
        _return: Option<(Span, String)>,
        throws: Vec<(Span, String)>
    },
    Argument {
        // TODO: Having optional names might not be a good thing, think about it
        // name: Option<(Span, String)>,
        name: (Span, String),
        kind: (Span, String)
    },
    Error {
        docstring: Vec<SpannedUnit>,
        parameters: Vec<SpannedUnit>,
        name: (Span, String),
        properties: Vec<SpannedUnit>,
        fields: Vec<SpannedUnit>
    },
    Validator {
        docstring: Vec<SpannedUnit>,
        properties: Vec<SpannedUnit>,
        name: (Span, String),
        expression_block: Box<SpannedUnit>
    },
    Field {
        docstring: Vec<SpannedUnit>,
        parameters: Vec<SpannedUnit>,
        // index: OrderIndex,
        optional: bool,
        name: String,
        kind: String,
        default_value: Option<String>,
    }
}


#[allow(unused)]
pub(crate) fn namespace(units: &Vec<SpannedUnit>) -> &String {
    let mut namespace: Option<&String> = None;

    for (_, unit) in units {
        if let ASTUnit::Namespace(_, n) = unit { namespace = Some(n) }
    }

    namespace.unwrap()
}

#[derive(PartialEq, Debug)]
pub enum UnitIndex {
    Index {
        path: String,
        source: String,
        // nodes: Vec<Unit>
    },
    Node {
        index: u32,
        start_position: u32, length: u32
    }
}

pub type SpannedUnit = (Span, ASTUnit);
pub type SourcedWhole = (CodeMap, Vec<SpannedUnit>);
pub type SourcedWholeRc = (CodeMap, Vec<Rc<SpannedUnit>>);


pub trait Details<'a> {
    fn find_namespace(&self) -> Option<&'a SpannedUnit> { todo!() }
    fn find_namespace_rc(&self) -> Option<&'a Rc<RefCell<SpannedUnit>>> { todo!() }
}

impl<'a> Details<'a> for &'a Vec<SpannedUnit> {
    fn find_namespace(&self) -> Option<&'a SpannedUnit> {
        for unit in self.iter() {
            if let ASTUnit::Namespace(_, _) = &unit.1 { return Some(unit) }
        }

        None
    }
}

impl<'a> Details<'a> for &'a Vec<Rc<SpannedUnit>> {
    fn find_namespace(&self) -> Option<&'a SpannedUnit> {
        for unit in self.iter() {
            if let ASTUnit::Namespace(_, _) = &unit.1 { return Some(unit) }
        }

        None
    }
}

impl<'a> Details<'a> for &'a Vec<Rc<RefCell<SpannedUnit>>> {
    fn find_namespace_rc(&self) -> Option<&'a Rc<RefCell<SpannedUnit>>> {
        for unit in self.iter() {
            if let ASTUnit::Namespace(_, _) = unit.borrow().1 { return Some(unit) }
        }

        None
    }
}

