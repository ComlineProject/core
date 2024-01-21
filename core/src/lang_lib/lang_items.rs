// Standard Uses
use std::path::Path;

// Local Uses
use crate::schema::idl::parser_new::from_path;
use crate::schema::idl::ast::unit::SourcedWhole;

// External Uses
use once_cell::sync::Lazy;


pub static LANGUAGE_ITEMS: Lazy<Vec<SourcedWhole>> = Lazy::new(||
    vec![
        from_path(Path::new("src/lang_lib/validators/string_bounds.ids")).unwrap()
    ]
);

