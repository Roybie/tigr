pub mod ast;
pub mod interpreter;
pub mod parser;
mod syntax;
mod scanner;

use interpreter::Eval;
use std::env;
use std::error::Error;
use std::io::Read;
use std::fs::File;
use std::path::Path;

fn main() {
    let args : Vec<String> = env::args().collect();
    if args.len() < 2 {
        panic!("Please provide tigr source file\n\nUsage: \"vl <sourcefile>\"");
    }
    let mut s = String::new();
    let path = Path::new(&args[1]);
    let display = path.display();

    let mut file = match File::open(&path) {
        Err(why) => panic!("Couldn't open {}: {}", display, why.description()),
        Ok(file) => file,
    };

    //TODO error checking here
    file.read_to_string(&mut s).unwrap();

    let tok = scanner::scan(&s);
    let parsed = parser::parse_Block(&s, tok).unwrap();
    println!("Parsed:\n{:?}\n", parsed);

    let mut e = Eval::new();
    let evaluated = e.eval(*parsed);

    println!("Program:\n{:?}\n", evaluated);
    e.print();
}

#[cfg(test)]
mod tests;
