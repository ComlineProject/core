// Standard Uses
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

// Local Uses
use crate::utils::codemap::{CodeMap, FileMap};
use crate::schema::idl::ast::unit::{ASTUnit, SourcedWholeRc, SpannedUnit};

// External Uses
use eyre::{bail, Result};
use pest::iterators::Pair;
use pest::Parser;
use pest_derive::Parser;


#[derive(Parser)]
#[grammar = "schema/idl/idl.pest"]
pub struct SchemaParser;


pub fn from_path(path: &Path) -> Result<SourcedWholeRc> {
    if !path.exists() { bail!("Path doesn't exist: {:?}", path) }
    let source = std::fs::read_to_string(path).unwrap();

    let sourced_whole = parse_source(
        source.clone(),
        path.file_name().unwrap().to_str().unwrap().to_owned()
    );

    sourced_whole
}

pub fn parse_source(source: String, name: String) -> Result<SourcedWholeRc> {
    let mut codemap = CodeMap::new();
    let file = codemap.insert_file(name, source.clone());

    let pairs = SchemaParser::parse(Rule::schema, source.as_str())?;
    let mut units = vec![];

    for pair in pairs {
        // TODO: Perhaps do error handling here with the results, as in stack them?
        if let Ok(unit) = parse_inner(pair, &file) {
            units.push(Rc::new(unit))
        }
    }

    Ok((codemap, units))
}

#[allow(unused)]
pub fn parse_inner(pair: Pair<'_, Rule>, file: &Arc<FileMap>) -> Result<SpannedUnit> {
    match pair.as_rule() {
        /*
        Rule::namespace => {
            let span = pair.as_span();
            let namespace_pair = pair.into_inner().next().unwrap();
            let namespace_span = namespace_pair.as_span();
            let namespace = namespace_pair.as_str().to_owned();

            Ok((
                file.insert_span(span.start(), span.end()),
                ASTUnit::Namespace(
                    file.insert_span(namespace_span.start(), namespace_span.end()),
                    namespace
                )
            ))
        },
        */
        Rule::import => {
            let span = pair.as_span();
            let import_pair = pair.into_inner().next().unwrap();
            let import_span = import_pair.as_span();
            let import = import_pair.as_str().to_owned();

            Ok((
                file.insert_span(span.start(), span.end()),
                ASTUnit::Import(
                    file.insert_span(import_span.start(), import_span.end()),
                    import
                )
            ))
        },
        Rule::settings => {
            let pair_span = pair.as_span();
            let unit_span = file.insert_span(pair_span.start(), pair_span.end());
            let inner = pair.into_inner();

            let mut docstring = vec![];
            let mut name = None;
            let mut parameters = vec![];

            for pair in inner {
                let span = pair.as_span();
                match pair.as_rule() {
                    Rule::docstring => docstring.push(to_docstring(pair, file)?),
                    Rule::id => name = Some((
                        file.insert_span(span.start(), span.end()),
                        pair.as_str().to_owned()
                    )),
                    Rule::parameter => parameters.push(to_parameter(pair, file)?),
                    missing => panic!("Rule not implemented in settings: {:?}", missing)
                }
            }

            Ok((unit_span, ASTUnit::Settings {
                docstring,
                name: name.unwrap(),
                parameters,
            }))
        },
        Rule::structure => {
            let pair_span = pair.as_span();
            let unit_span = file.insert_span(pair_span.start(), pair_span.end());
            let inner = pair.into_inner();

            let mut docstring = vec![];
            let parameters = vec![];
            let mut name = None;
            let mut fields = vec![];

            for pair in inner {
                let span = pair.as_span();
                match pair.as_rule() {
                    Rule::docstring => docstring.push(to_docstring(pair, file)?),
                    Rule::id => name = Some((
                        file.insert_span(span.start(), span.end()),
                        pair.as_str().to_owned()
                    )),
                    Rule::field => fields.push(to_field(pair, file)?),
                    missing => panic!("Rule not implemented in structure: {:?}", missing)
                }
            }

            Ok((unit_span, ASTUnit::Struct {
                docstring, parameters, name: name.unwrap(), fields,
            }))
        }
        Rule::constant => {
            let pair_span = pair.as_span();
            let unit_span = file.insert_span(pair_span.start(), pair_span.end());

            let mut name = None;
            let mut kind = None;
            let mut default_value = None;

            for pair in pair.into_inner() {
                let span = pair.as_span();
                match pair.as_rule() {
                    Rule::id => {
                        name = Some((
                            file.insert_span(span.start(), span.end()),
                            pair.as_str().to_owned()
                        ));
                    },
                    Rule::kind => {
                        kind = Some((
                            file.insert_span(span.start(), span.end()),
                            pair.as_str().to_owned()
                        ))
                    },
                    Rule::value => {
                        default_value = Some((
                            file.insert_span(span.start(), span.end()),
                            pair.as_str().to_owned()
                        ))
                    },
                    missing => panic!("Rule not implemented for constants: '{:?}'", missing)
                }
            }

            Ok((unit_span, ASTUnit::Constant {
                    docstring: vec![],
                    name: name.unwrap(), kind: kind.unwrap(),
                    default_value
                }
            ))
        },
        Rule::enumeration => {
            let pair_span = pair.as_span();
            let unit_span = file.insert_span(pair_span.start(), pair_span.end());
            let mut inner = pair.into_inner();

            let docstring = vec![];
            let mut name = None;
            let mut variants = vec![];

            for pair in inner {
                let span = pair.as_span();
                match pair.as_rule() {
                    Rule::id => name = Some((
                        file.insert_span(span.start(), span.end()),
                        pair.as_str().to_owned()
                    )),
                    Rule::enum_variant => variants.push(to_enum_variant(pair, file)?),
                    missing => panic!("Rule not implemented for enumeration: '{:?}'", missing)
                }
            }

            Ok((unit_span, ASTUnit::Enum { docstring, name: name.unwrap(), variants, }))
        }
        Rule::error => {
            let pair_span = pair.as_span();
            let unit_span = file.insert_span(pair_span.start(), pair_span.end());
            let mut inner = pair.into_inner();

            let mut docstring = vec![];
            let mut parameters = vec![];
            let mut name = None;
            let properties = vec![];
            let mut fields = vec![];

            for pair in inner {
                let span = pair.as_span();
                match pair.as_rule() {
                    Rule::docstring => docstring.push(to_docstring(pair, file)?),
                    Rule::parameter => parameters.push(to_parameter(pair, file)?),
                    Rule::id => name = Some((
                        file.insert_span(span.start(), span.end()),
                        pair.as_str().to_owned()
                    )),
                    Rule::property => parameters.push(to_property(pair, file)?),
                    Rule::field => fields.push(to_field(pair, file)?),
                    missing => panic!("Rule not implemented in error: {:?}", missing)
                }
            }

            Ok((unit_span, ASTUnit::Error {
                docstring, parameters, name: name.unwrap(), fields, properties,
            }))
        }
        Rule::protocol => {
            let pair_span = pair.as_span();
            let unit_span = file.insert_span(pair_span.start(), pair_span.end());
            let mut inner = pair.into_inner();

            let mut docstring = vec![];
            let mut parameters = vec![];
            let mut name = None;
            let mut functions = vec![];

            for pair in inner {
                let span = pair.as_span();
                let next = &file.next_id;
                match pair.as_rule() {
                    Rule::docstring => docstring.push(to_docstring(pair, file)?),
                    Rule::property => parameters.push(to_parameter(pair, file)?),
                    Rule::id => {
                        name = Some((
                            file.insert_span(span.start(), span.end()),
                            pair.as_str().to_owned()
                        ))
                    },
                    Rule::function => functions.push(to_function(pair, file)?),
                    missing => panic!("Rule not implemented in 'protocol': {:?}", missing)
                }
            }

            Ok((unit_span, ASTUnit::Protocol {
                docstring, parameters, name: name.unwrap(), functions
            }))
        },
        missing => panic!("Rule not implemented: {:?}", missing)
    }
}


pub fn to_docstring(pair: Pair<'_, Rule>, file: &Arc<FileMap>) -> Result<SpannedUnit> {
    let pair = pair.into_inner().next().unwrap();

    match pair.as_rule() {
        Rule::docstring_property => {
            let pair_span = pair.as_span();
            let unit_span = file.insert_span(pair_span.start(), pair_span.end());
            let pairs = pair.into_inner();

            let mut variable: Option<String> = None;
            let mut description: Option<String> = None;

            for pair in pairs {
                match pair.as_rule() {
                    Rule::domain => variable = Some(pair.as_str().to_owned()),
                    Rule::docstring_description => description = Some(pair.as_str().to_owned()),
                    r => panic!("Rule not implemented: {:?}", r)
                }
            }

            Ok((unit_span, ASTUnit::Docstring {
                variable, description: description.unwrap(),
            }))
        },
        Rule::docstring_description => {
            let pair_span = pair.as_span();
            let unit_span = file.insert_span(pair_span.start(), pair_span.end());

            return Ok((unit_span, ASTUnit::Docstring {
                variable: None, description: pair.as_str().to_owned()
            }))
        },
        r => panic!("Rule not implemented: {:?}", r)
    }
}


#[allow(unused)]
pub fn to_parameter(pair: Pair<'_, Rule>, file: &Arc<FileMap>) -> Result<SpannedUnit> {
    let pair_span = pair.as_span();
    let unit_span = file.insert_span(pair_span.start(), pair_span.end());
    let mut inner = pair.into_inner();

    let name_pair = inner.next().unwrap();
    let name_span = name_pair.as_span();
    let default_value = inner.next().unwrap();
    let default_value_span = default_value.as_span();

    Ok((unit_span, ASTUnit::Parameter {
        name: (
            file.insert_span(name_span.start(), name_span.end()),
            name_pair.as_str().to_owned()
        ),
        default_value: (
            file.insert_span(default_value_span.start(), default_value_span.end()),
            default_value.as_str().to_owned()
        )
    }))
}


#[allow(unused)]
pub fn to_field(pair: Pair<'_, Rule>, file: &Arc<FileMap>) -> Result<SpannedUnit> {
    let pair_span = pair.as_span();
    let unit_span = file.insert_span(pair_span.start(), pair_span.end());
    let mut inner = pair.into_inner();

    let mut docstring = vec![];
    let mut parameters = vec![];
    let mut optional: bool = false;
    let mut name: Option<String> = None;
    let mut kind: Option<String> = None;
    let mut default_value: Option<String> = None;

    for pair in inner {
        match pair.as_rule() {
            Rule::docstring => docstring.push(to_docstring(pair, file)?),
            Rule::parameter => {} // parameters.push(to_parameter(pair, file)),
            Rule::property => {} //
            Rule::requirement => optional = true,
            Rule::id => name = Some(pair.as_str().to_owned()),
            Rule::kind => kind = Some(pair.as_str().to_owned()),
            Rule::value => default_value = Some(pair.as_str().to_owned()),
            r => panic!("Rule not implemented in 'field': {:?}", r)
        }
    }

    Ok((unit_span, ASTUnit::Field {
        docstring, parameters,
        optional, name: name.unwrap(), kind: kind.unwrap(), default_value,
    }))
}

pub fn to_property(pair: Pair<'_, Rule>, file: &Arc<FileMap>) -> Result<SpannedUnit> {
    let pair_span = pair.as_span();
    let unit_span = file.insert_span(pair_span.start(), pair_span.end());
    let inner = pair.into_inner().next().unwrap();

    let mut name = None;
    let mut expression = None;

    let span = inner.as_span();
    match inner.as_rule() {
        Rule::property_domain => name = Some((
            file.insert_span(span.start(), span.end()),
            inner.as_str().to_string()
        )),
        Rule::property_expression => expression = Some((
            file.insert_span(span.start(), span.end()),
            inner.as_str().to_string()
        )),
        missing => panic!("Rule not implemented in 'property': {:?}", missing)
    }

    Ok((unit_span, ASTUnit::Property { name: name.unwrap(), expression }))
}


#[allow(unused)]
fn to_enum_variant(pair: Pair<'_, Rule>, file: &Arc<FileMap>) -> Result<SpannedUnit> {
    let pair_span = pair.as_span();
    let unit_span = file.insert_span(pair_span.start(), pair_span.end());
    let inner = pair.into_inner();

    let mut name = None;
    let kind = None;

    for pair in inner {
        let span = pair.as_span();
        match pair.as_rule() {
            Rule::id => name = Some((
                file.insert_span(span.start(), span.end()),
                pair.as_str().to_owned()
            )),
            missing => panic!("Rule not implemented for enum_variant: {:?}", missing)
        }
    }

    Ok((unit_span, ASTUnit::EnumVariant {
        name: name.unwrap(), kind,
    }))
}

#[allow(unused)]
fn to_function(pair: Pair<'_, Rule>, file: &Arc<FileMap>) -> Result<SpannedUnit> {
    let pair_span = pair.as_span();
    let unit_span = file.insert_span(pair_span.start(), pair_span.end());
    let inner = pair.into_inner();

    let mut name = None;
    let asynchronous = None;
    let docstring = vec![];
    let mut parameters = vec![];
    let mut arguments = vec![];
    let mut _return = None;
    let mut throws = vec![];

    for pair in inner {
        let span = pair.as_span();
        match pair.as_rule() {
            Rule::property => parameters.push(to_parameter(pair, file)?),
            Rule::id => name = Some((
                file.insert_span(span.start(), span.end()),
                pair.as_str().to_owned()
            )),
            Rule::argument => {
                let pair_span = pair.as_span();
                let mut inner = pair.into_inner();
                let unit_span = file.insert_span(pair_span.start(), pair_span.end());
                let name_pair = inner.next().unwrap();
                let name_span = name_pair.as_span();
                let kind_pair = inner.next().unwrap();
                let kind_span = kind_pair.as_span();

                arguments.push((unit_span, ASTUnit::Argument {
                    name: (
                        file.insert_span(name_span.start(), name_span.end()),
                        name_pair.as_str().to_owned()
                    ),
                    kind: (
                        file.insert_span(kind_span.start(), kind_span.end()),
                        kind_pair.as_str().to_owned()
                    ),
                }))
            },
            Rule::returns => {
                let mut inner = pair.into_inner();
                let name_pair = inner.next().unwrap();
                let name_span = name_pair.as_span();

                _return = Some((
                    file.insert_span(name_span.start(), name_span.end()),
                    name_pair.as_str().to_owned()
                ));
            },
            Rule::throws => {
                let mut inner = pair.into_inner();
                let name_pair = inner.next().unwrap();
                let name_span = name_pair.as_span();

                throws.push((
                    file.insert_span(name_span.start(), name_span.end()),
                    name_pair.as_str().to_owned()
                ));
            }
            missing => panic!("Rule not implemented for function: {:?}", missing)
        }
    }

    Ok((unit_span, ASTUnit::Function {
        docstring, parameters,
        name: name.unwrap(), asynchronous,
        arguments, _return, throws
    }))
}

