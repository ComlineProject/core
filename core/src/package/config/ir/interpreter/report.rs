// Standard Uses

// Crate Uses

// External Uses

/*
#[allow(unused)]
pub fn report_errors(
    spec_version: Option<u32>, schema_paths: Option<Vec<String>>,
    dependencies: Option<Vec<Dependency>>, assignments: Vec<AssignmentUnit>
) -> Result<(), dyn snafu::Error> {
    // let mut errors = vec![];

    // let spec_version = spec_version.context(report::SpecVersionNotToldSnafu);
    // let spec_version = spec_version.context(report::SpecVersionNotToldSnafu);
    // spec_version.unwrap();

    Ok(())

    // spec_version.context(|e| report::SpecVersionNotToldSnafu );

    /*
    if spec_version.is_none() {
        ensure!(
            spec_version.is_none(),
            ReportDetails { kind: CompileError::SpecVersionNotTold, span: None }
        );
       errors.push();
    }
    */

    // errors
}

#[allow(unused)]
pub fn report_warnings(
    spec_version: Option<u32>, schema_paths: Option<Vec<String>>,
    dependencies: Option<Vec<Dependency>>, assignments: Vec<AssignmentUnit>
) -> Vec<ReportDetails<CompileWarning>> {
    let warnings = vec![];


    warnings
}
*/
