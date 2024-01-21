// Standard Uses
use std::sync::Arc;

// Local Uses
use crate::schema::idl::ast::unit::{ASTUnit, SpannedUnit};
use crate::utils::codemap::FileMap;

// External Uses
use pest::iterators::Pair;
use pest_derive::Parser;


#[derive(Parser)]
#[grammar = "schema/idl/idl.pest"]
pub struct IDLParser;

/*
pub fn parse_inner(pairs: Pair<Rule>) -> Result<ASTUnit> {
    match pairs.as_rule() {
        Rule::namespace => {
            Ok(ASTUnit::Namespace(pairs.into_inner().as_str().to_owned()))
        },
        Rule::import => {
            Ok(ASTUnit::Import(pairs.into_inner().as_str().to_owned()))
        }
        Rule::constant => {
            let pairs = pairs.into_inner();

            let mut docstrings: Vec<ASTUnit> = vec![];
            let mut id: Option<String> = None;
            let mut kind: Option<String> = None;
            let mut default_value: Option<String> = None;

            for pair in pairs {
                match pair.as_rule() {
                    Rule::docstring => docstrings.push(to_docstring(pair)),
                    Rule::id => id = Some(pair.as_str().to_owned()),
                    Rule::kind => kind = Some(pair.as_str().to_owned()),
                    Rule::value => default_value = to_value_other(pair),
                    missing => panic!("Rule not implemented on 'constant': {:?}", missing)
                }
            }

            Ok(ASTUnit::Constant {
                docstring: docstrings,
                name: id.ok_or("Id is not present").unwrap(),
                kind: kind.ok_or("Type is not present").unwrap(),
                default_value,
            })
        },
        Rule::settings => {
            let pairs = pairs.into_inner();

            let mut docstrings: Vec<ASTUnit> = vec![];
            let mut name: Option<String> = None;
            let mut parameters: Vec<ASTUnit> = vec![];

            for pair in pairs {
                match pair.as_rule() {
                    Rule::docstring => docstrings.push(to_docstring(pair)),
                    Rule::id => name = Some(pair.as_str().to_owned()),
                    Rule::parameter => parameters.push(to_parameter(pair)),
                    missing => panic!("Rule not implemented on 'settings': {:?}", missing)
                }
            }

            Ok(ASTUnit::Settings {
                docstring: docstrings,
                name: name.unwrap(),
                parameters,
            })
        },
        Rule::enumeration => {
            let pairs = pairs.into_inner();

            let mut docstrings: Vec<ASTUnit> = vec![];
            let mut name: Option<String> = None;
            let mut variants: Vec<ASTUnit> = vec![];

            for pair in pairs {
                match pair.as_rule() {
                    Rule::docstring => docstrings.push(to_docstring(pair)),
                    Rule::id => name = Some(pair.as_str().to_owned()),
                    Rule::enum_variant => {
                        let mut inner = pair.into_inner();
                        let name = inner.next().unwrap().as_str().to_string();
                        let kind = inner.next().map(|s| s.as_str().to_string());

                        variants.push(ASTUnit::EnumVariant {
                            name, kind,
                        });
                    },
                    missing => panic!("Rule not implemented on 'enumeration': {:?}", missing)
                }
            }

            Ok(ASTUnit::Enum {
                docstring: docstrings,
                name: name.unwrap(), variants,
            })
        },
        Rule::structure => {
            let pairs = pairs.into_inner();

            let mut docstrings: Vec<ASTUnit> = vec![];
            let mut parameters: Vec<ASTUnit> = vec![];
            let mut name: Option<String> = None;
            let mut fields: Vec<ASTUnit> = vec![];

            for pair in pairs {
                match pair.as_rule() {
                    Rule::docstring => docstrings.push(to_docstring(pair)),
                    Rule::id => name = Some(pair.as_str().to_owned()),
                    Rule::parameter => parameters.push(to_parameter(pair)),
                    Rule::field => fields.push(to_field(pair)),
                    missing => panic!("Rule not implemented on 'structure': {:?}", missing)
                }
            }

            Ok(ASTUnit::Struct {
                docstring: docstrings, parameters,
                name: name.unwrap(), fields: vec![],
            })
        },
        Rule::validator => {
            let pairs = pairs.into_inner();

            let mut docstrings: Vec<ASTUnit> = vec![];
            let mut properties: Vec<ASTUnit> = vec![];
            let mut name: Option<String> = None;
            let mut expression_block: Option<ASTUnit> = None;

            for pair in pairs {
                match pair.as_rule() {
                    Rule::docstring => docstrings.push(to_docstring(pair)),
                    Rule::property => properties.push(to_property(pair)),
                    Rule::id => name = Some(pair.as_str().to_owned()),
                    Rule::expression_block =>
                        expression_block = Some(to_expression_block(pair)),
                    missing => panic!("Rule not implemented on 'validator': {:?}", missing)
                }
            }

            Ok(ASTUnit::Validator {
                docstring: docstrings, properties,
                name: name.unwrap(), expression_block: Box::from(expression_block.unwrap()),
            })
        },
        Rule::protocol => {
            let pairs = pairs.into_inner();

            let mut docstrings = vec![];
            let mut parameters = vec![];
            let mut name: Option<String> = None;
            let mut functions = vec![];

            for pair in pairs {
                match pair.as_rule() {
                    Rule::docstring => docstrings.push(to_docstring(pair)),
                    Rule::property => parameters.push(to_property(pair)),
                    Rule::id => name = Some(pair.as_str().to_owned()),
                    Rule::function => functions.push(to_function(pair)),
                    missing => panic!("Rule not implemented on 'Protocol': {:?}", missing)
                }
            }

            Ok(ASTUnit::Protocol {
                docstring: docstrings,
                parameters,
                name: name.unwrap(),
                functions,
            })
        },
        r => panic!("Rule not implemented: {:?}", r)
    }
}
*/

pub fn to_docstring(pair: Pair<'_, Rule>, file: &Arc<FileMap>) -> SpannedUnit {
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

            (unit_span, ASTUnit::Docstring {
                variable, description: description.unwrap(),
            })
        },
        Rule::docstring_description => {
            let pair_span = pair.as_span();
            let unit_span = file.insert_span(pair_span.start(), pair_span.end());

            return (unit_span, ASTUnit::Docstring {
                variable: None, description: pair.as_str().to_owned()
            })
        },
        r => panic!("Rule not implemented: {:?}", r)
    }
}

/*
pub fn to_parameters(mut pairs: Pairs<Rule>) -> Vec<Unit> {
    let mut params = vec![];

    let t = pairs.as_str();
    while let Some(pair) = pairs.next() {
        let temp = pair.as_str();

        params.push(to_parameter(pair.into_inner()));
    }

    params
}
*/

/*
pub fn to_parameters(mut pairs: Pairs<Rule>) -> Vec<Parameter> {
    let mut params = vec![];

    while let Some(pair) = pairs.next() {
        params.push(to_parameter(pair.into_inner()));
    }

    params
}
*/

pub fn to_value_other(pair: Pair<'_, Rule>) -> Option<String> {
    let inner = pair.into_inner().next().unwrap();

    match inner.as_rule() {
        Rule::string => Some(inner.as_str().to_string()),
        Rule::string_interpolated => Some(inner.as_str().to_string()),
        Rule::number => Some(inner.as_str().to_string()),
        r => panic!("Rule not implemented in 'value': {:?}", r)
    }

    // "".to_string()
}

#[allow(unused)]
pub fn to_field(pair: Pair<'_, Rule>, file: &Arc<FileMap>) -> ASTUnit {
    let pairs = pair.into_inner();

    let mut docstring = vec![];
    let mut parameters = vec![];
    let mut optional: bool = false;
    let mut name: Option<String> = None;
    let mut kind: Option<String> = None;
    let mut default_value: Option<String> = None;

    for pair in pairs {
        match pair.as_rule() {
            Rule::docstring => docstring.push(to_docstring(pair, file)),
            Rule::parameter => {} // parameters.push(to_parameter(pair, file)),
            Rule::requirement => optional = true,
            Rule::id => name = Some(pair.as_str().to_owned()),
            Rule::kind => kind = Some(pair.as_str().to_owned()),
            Rule::value => default_value = Some(pair.as_str().to_owned()),
            r => panic!("Rule not implemented in 'field': {:?}", r)
        }
    }

    ASTUnit::Field {
        docstring, parameters,
        optional, name: name.unwrap(), kind: kind.unwrap(), default_value,
    }
}

/*
pub fn to_property(pair: Pair<Rule>, file: &Arc<FileMap>) -> ASTUnit {
    let inner = pair.into_inner().next().unwrap();

    let mut name: Option<String> = None;
    let mut expression: Option<String> = None;

    match inner.as_rule() {
        Rule::property_domain => name = Some(inner.as_str().to_string()),
        Rule::property_expression => expression = Some(inner.as_str().to_string()),
        r => panic!("Rule not implemented in 'property': {:?}", r)
    }

    ASTUnit::Property { name: name.unwrap(), expression }
}
*/

#[allow(unused)]
fn to_function(pair: Pair<'_, Rule>) -> ASTUnit {
    let inner = pair.into_inner();

    let mut synchronous = true;
    // let mut direction = Direction::Both;
    let mut name: Option<String> = None;
    let arguments: Vec<ASTUnit> = vec![];
    // let mut properties = vec![];
    let mut returns = vec![];
    // let throws = vec![];

    for pair in inner {
        match pair.as_rule() {
            // Rule::index => index = Some(pair.as_str().parse().unwrap()),
            Rule::property => {} // properties.push(to_property(pair)),
            Rule::id => name = Some(pair.as_str().to_owned()),
            /*
            Rule::direction => direction = match pair.as_str() {
                "client" => Direction::Client,
                "server" => Direction::Server,
                dir => panic!("Direction {:#} does not exist", dir)
            },
            */
            Rule::asynchronous => synchronous = false,
            /*
            Rule::argument => {
                arguments.push(to_argument(pair.into_inner()))
            }
            */
            Rule::returns => {
                returns.push(pair.into_inner().next().unwrap().as_str().to_owned());
            }
            Rule::parameter => {
                panic!()
            }
            Rule::throws => {
                panic!()
            }
            r => panic!("Rule not implemented in 'function': {:?}", r)
        }
    }

    todo!()
    /*
    ASTUnit::Function {
        docstring: vec![],
        name: name.unwrap(),
        asynchronous,
        arguments,
        returns,
        throws,
    }
    */
}

/*
fn to_argument(pairs: Pairs<Rule>) -> Argument {
    let mut id: Option<String> = None;
    let mut kind: Option<TypeValue> = None;

    for pair in pairs {
        match pair.as_rule() {
            Rule::id => id = Some(pair.as_str().to_owned()),
            Rule::kind => kind = Some(primitive::to_type(pair.as_str())),
            _ => unreachable!()
        }
    }

    Argument { id, type_: kind.unwrap() }
}
*/

pub fn to_expression_block(pair: Pair<'_, Rule>) -> ASTUnit {
    let inner = pair.into_inner().next().unwrap();

    // let mut expression = vec![];
    let function_calls: Vec<String> = vec![];

    match inner.as_rule() {
        Rule::function_call => {
            // expression.push()
        },
        r => panic!("Rule not implemented in 'expression_block': {:?}", r)
    }

    ASTUnit::ExpressionBlock {
        function_calls
    }
}
