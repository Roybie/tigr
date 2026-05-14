//! Tigr CLI.
//!
//! Usage: `tigr <file.tg>`
//!
//! The `--legacy` flag is reserved for re-enabling the v0.1 tree-walking
//! interpreter once `src/v01/` is wired back into the build.

use std::path::Path;
use std::process::ExitCode;

mod v01;
mod vm;

#[cfg(test)]
mod tests;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let mut filename: Option<&str> = None;
    let mut legacy = false;
    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "--legacy" => legacy = true,
            "-h" | "--help" => {
                print_usage();
                return ExitCode::SUCCESS;
            }
            other => filename = Some(other),
        }
    }
    let Some(filename) = filename else {
        print_usage();
        return ExitCode::FAILURE;
    };
    if legacy {
        eprintln!(
            "tigr: --legacy mode is not currently wired in. \
             See src/v01/mod.rs for restoration instructions."
        );
        return ExitCode::FAILURE;
    }
    match vm::run_file(Path::new(filename)) {
        Ok(value) => {
            println!("{value:?}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    eprintln!("usage: tigr <file.tg>");
    eprintln!("       tigr --legacy <file.tg>   (v0.1 interpreter; not currently wired)");
}
