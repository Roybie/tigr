extern crate lalrpop_util;

pub mod ast;
pub mod interpreter;
pub mod parser;
mod syntax;
mod lexer;

use interpreter::Eval;
use std::env;
use std::error::Error;
use std::io::Read;
use std::fs::File;
use std::path::Path;

use lalrpop_util::ParseError;

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

    let lexer = lexer::Lexer::new(&s);
    match parser::parse_Block(lexer){
        Ok(s) => { output_success(s); },
        Err(e) => { output_error(e, &s); },
    };
}

fn output_success(parsed: Box<ast::Expr>) {
    println!("Parsed:\n{:?}\n", parsed);

    let mut e = Eval::new();
    let evaluated = e.eval(*parsed);

    println!("Program:\n{:?}\n", evaluated);
    e.print();
}

fn output_error(error: ParseError<usize, syntax::Token, lexer::LexicalError>, source: &str) {
    match error {
        ParseError::User{ error: lexer::LexicalError::InvalidToken(line, token, char_index) } |
        ParseError::UnrecognizedToken{ token: Some((line, token, char_index)), expected:_ } => {
            let mut char_index = char_index;
            let mut error_line = "";
            for (i, lin) in source.lines().enumerate() {
                if i == line - 1 {
                    error_line = lin;
                    break;
                }
                char_index -= lin.len() + 1;
            }
            println!("Unexpected Character {:?} on line: {}\n", token, line);
            println!("{}", error_line);
            println!("{:indent$}└> Unexpected Character", "", indent=char_index);
        },
        e => println!("{:?}", e),
    }
}

#[cfg(test)]
mod tests;
