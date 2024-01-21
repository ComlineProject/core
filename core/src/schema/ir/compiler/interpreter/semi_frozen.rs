// Standard Uses

// Crate Uses
use crate::utils::codemap::Span;

// External Uses


#[allow(unused)]
#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub(crate) namespace: (Span, String),
    // TODO: Alias of imports for schemas, objects, etc, is needed for namespace conflicts
    // pub(crate) alias: (Span, String)
}

// TODO: All of these structures might be able to be replace with a single one, since
//       a lot of them might just need to have a name and span to the objects
#[derive(Debug, Clone, PartialEq)]
pub struct Structure {
    pub(crate) name: (Span, String)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Constant {
    pub(crate) name: (Span, String)
}

#[allow(unused)]
#[derive(Debug, Clone, PartialEq)]
pub struct Protocol {
    pub(crate) name: (Span, String)
}

