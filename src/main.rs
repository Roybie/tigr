pub mod ast;
pub mod parser;

fn main() {
    println!("{:?}", parser::parse_Block(r#"
    for (enum, iter, 0..100:10) { #true };
    for (iter, 0..100:10) { #true };
    for (0..100) { #true }
    "#).unwrap());
}

#[cfg(test)]
mod tests;
