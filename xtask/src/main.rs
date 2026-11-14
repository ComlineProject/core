//! `cargo xtask <task>`: the repository's own checks (core#268, step 1.2).

use std::process::ExitCode;

use xtask::repo::Repo;
use xtask::{bump, seal};

const USAGE: &str = "\
usage: cargo xtask <task>

  check-format-bump [--base <rev>]   what a format change has to come with: with --base,
                                     a bump of CURRENT_IR_FORMAT_VERSION needs its step,
                                     directory, seal and fixtures, and moved golden hashes
                                     need a ledger row; always, the seals hold
  check-seal                         the seals in core/formats/MANIFEST hold
  seal-format <N> --why <text>       record the seal of the superseded format N, or move it";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let repo = Repo::here();
    let result = match args.first().map(String::as_str) {
        Some("check-format-bump") => flag(&args, "--base")
            .and_then(|base| bump::check(&repo, base.as_deref()))
            .map(|problems| report("the format gate", problems)),
        Some("check-seal") => seal::verify(&repo).map(|problems| report("the seals", problems)),
        Some("seal-format") => seal_format(&repo, &args),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

/// The value after `name` in `args`, if the flag is there.
fn flag(args: &[String], name: &str) -> Result<Option<String>, String> {
    match args.iter().position(|a| a == name) {
        None => Ok(None),
        Some(at) => args
            .get(at + 1)
            .cloned()
            .map(Some)
            .ok_or(format!("{name} needs a value")),
    }
}

fn report(what: &str, problems: Vec<String>) -> ExitCode {
    if problems.is_empty() {
        println!("{what}: ok");
        return ExitCode::SUCCESS;
    }
    for problem in &problems {
        eprintln!("error: {problem}");
    }
    eprintln!("{what}: {} problem(s)", problems.len());
    ExitCode::FAILURE
}

fn seal_format(repo: &Repo, args: &[String]) -> Result<ExitCode, String> {
    let format: u32 = args
        .get(1)
        .and_then(|n| n.parse().ok())
        .ok_or("seal-format needs a format number")?;
    let why = flag(args, "--why")?.ok_or("a seal is moved with a reason: `--why <text>`")?;
    seal::seal(repo, format, &why)?;
    println!("sealed format {format}");
    Ok(ExitCode::SUCCESS)
}
