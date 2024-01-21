// Standard Uses
use std::cell::Ref;
use std::ops::Range;

// Crate Uses
use crate::schema::ir::context::SchemaContext;
use crate::utils::codemap::Span;

// External Uses


#[derive(Debug)]
pub struct ReportDetails {
    pub line: Range<usize>,
    pub pos: Range<usize>
}

impl ReportDetails {
    pub(crate) fn fetch(
        schema_context: &Ref<'_, SchemaContext>, span: &Span
    ) -> Option<Self> {
        let pos = schema_context.schema.0.files().first()
            .unwrap().range_of(*span).unwrap();

        Some(Self { line: Default::default(), pos })
    }
}

/*
#[allow(unused)]
pub fn report_errors<'a>(context: SchemaContext<'_>, errors: Vec<ReportDetails<CompileError>>) {
    let out = Color::Fixed(81);

    /*
    errors.into_iter().for_each(|e| {
        let span = context.
        Report::build(ReportKind::Error, &provenance.0, 12)
            .with_code(e.code()).with_message(format!("{:?}", e.kind))
            .with_label(
                Label::new((&provenance.0, e.span.unwrap()...e.span.unwrap().end))
                    .with_message(format!("This is of type {:?}", e.kind.fg(out)))
                    .with_color(out),
            )
            .finish().print(&mut source)
            .unwrap();
    });
    */
}


#[allow(unused)]
pub fn report_warnings(context: SchemaContext<'_>, warnings: Vec<ReportDetails<CompileError>>) {
    todo!()
}

*/

