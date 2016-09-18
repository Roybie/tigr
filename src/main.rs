pub mod ast;
pub mod parser;

fn main() {
    println!("{:?}", parser::parse_Block(r#"
    8 * (a = for[](i,j,6_8:1) { i }) ; 5 + (6;9;10;)
    "#).unwrap());
}
