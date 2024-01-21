// Standard Uses

// Crate Uses
use crate::report::ReportDetails;
use crate::utils::codemap::Span;

// External Uses


#[allow(unused)]
pub(crate) fn colliding_namespace_err(
    primary_schema: (), relative_schemas: &[()],
) -> Result<(), ReportDetails> {
    /*
    Err(ReportDetails {
        line: Default::default(),
        kind: "Namespace".to_string(),
        message: format!(
            "Namespace {} used in another schema: {} and {},\n \
                            only one schema per namespace is allowed",
            n, context.main.0.filename(), unit_namespace.unwrap().0.filename()
        ),
        start: 0, end: 0,
        pos: Default::default(),
    })
    */

    todo!()
}

#[allow(unused)]
pub(crate) fn colliding_namespace_other_err() {
    /*
    let mut found: Option<&SchemaContext> = None;

    for relative_ctx in schema_ctx.borrow().schema_contexts.iter() {
        if unit::namespace(&relative_ctx.schema.1) == n {
            /*
            if found.is_some() {
                return Err(ReportDetails {
                    kind: "namespace".to_string(),
                    message: format!(
                        "Found namespace {} when its already declared in {}",
                        &n, &relative_ctx.main.0.filename()
                    ),
                    start: 0, end: 0,
                })
            }
            */

            found = Some(relative_ctx)
        }
    }
    */
}

#[allow(unused)]
pub(crate) fn const_already_defined(
    this: &(Span, String), other: &(Span, String)
) -> Result<(), ReportDetails> {
    todo!()
}

#[allow(unused)]
pub(crate) fn object_name_collision(
    this: &(Span, String), other: &(Span, String)
) -> Result<(), ReportDetails> {
    todo!()
}

