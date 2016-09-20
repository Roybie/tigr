pub mod ast;
pub mod interpreter;
pub mod parser;
mod syntax;
mod scanner;

use interpreter::Eval;

fn main() {
    let src = r"
    a = for[] (e,i,0..10:2) { i*e };
    b = [if true { [1,2,3] },8*8]
    ";
    let tok = scanner::scan(src);
    let parsed = parser::parse_Block(src, tok).unwrap();
    println!("Parsed:\n{:?}\n", parsed);

    let mut e = Eval::new();
    let evaluated = e.eval(*parsed);

    println!("Program:\n{:?}\n", evaluated);
    e.print();
}

#[cfg(test)]
mod tests;
