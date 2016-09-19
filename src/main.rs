pub mod ast;
pub mod parser;
mod syntax;
mod scanner;

fn main() {
    let src = r#"
        for (enum, iter, 0..100:10) { true };
        for (iter, 0..100:10) { true };
        for (0..100) { true }
    "#;
    let tok = scanner::scan(src);
    println!("{:?}", parser::parse_Block(src, tok));
}

#[cfg(test)]
mod tests;
