//! `tigr disasm <file.tg> [-r|--nested]` — compile a program and print
//! its bytecode listing without running it. With `-r`, recurse into
//! nested function chunks too. The inspection tool for the v0.12
//! constant-folding / peephole work.

use std::path::Path;
use std::process::ExitCode;

use crate::vm::{self, source_map::SourceMap};

pub fn run(args: &[String]) -> ExitCode {
    let mut path: Option<&str> = None;
    let mut recursive = false;
    for arg in args {
        match arg.as_str() {
            "-r" | "--nested" => recursive = true,
            other if path.is_none() => path = Some(other),
            _ => {}
        }
    }
    let Some(path) = path else {
        eprintln!("usage: tigr disasm <file.tg> [-r|--nested]");
        return ExitCode::FAILURE;
    };

    let mut sources = SourceMap::new();
    let main = match vm::compile_file_into(Path::new(path), &mut sources) {
        Ok(func) => func,
        Err(err) => {
            eprintln!("{}", err.render(&sources));
            return ExitCode::FAILURE;
        }
    };

    let listing = if recursive {
        main.chunk.disassemble_recursive("<main>")
    } else {
        main.chunk.disassemble("<main>")
    };
    print!("{listing}");
    ExitCode::SUCCESS
}
