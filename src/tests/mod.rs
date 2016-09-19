use ast::*;
use parser::*;

macro_rules! e {
    ($t:ident, $($e:expr),+) => (Box::new(Expr::$t($($e),+)));
}

macro_rules! t {
    ($t:ident) => (Box::new(Expr::Type(Type::$t)));
    ($t:ident, $e:expr) => (Box::new(Expr::Type(Type::$t($e))));
}

#[test]
fn precidence_unary() {
    //unary before anything
    assert_eq!(
        parse_Block("1 - -2").unwrap(),
        e!(BinOp, t!(Number, 1), BinOpCode::Sub, e!(UnOp, UnOpCode::Neg, t!(Number, 2)))
    );

    assert_eq!(
        parse_Block("a = !#true").unwrap(),
        e!(BinOp, t!(Id, "a".to_owned()), BinOpCode::Ass, e!(UnOp, UnOpCode::Not, t!(Bool, true)))
    );
}

#[test]
fn precidence_binary() {
    // Mul/div before add/sub
    assert_eq!(
        parse_Block("1 + 2 * 3").unwrap(), // 1 + (2 * 3)
        e!(BinOp, t!(Number, 1), BinOpCode::Add, e!(BinOp, t!(Number, 2), BinOpCode::Mul, t!(Number, 3)))
    );
    assert_eq!(
        parse_Block("1 - 2 / 3").unwrap(), // 1 - (2 / 3)
        e!(BinOp, t!(Number, 1), BinOpCode::Sub, e!(BinOp, t!(Number, 2), BinOpCode::Div, t!(Number, 3)))
    );

    //Add sub together, left associative
    assert_eq!(
        parse_Block("1 - 2 + 3").unwrap(), // (1 - 2) + 3
        e!(BinOp, (e!(BinOp, t!(Number, 1), BinOpCode::Sub, t!(Number, 2))), BinOpCode::Add, t!(Number, 3))
    );

    //mul div together, left associative
    assert_eq!(
        parse_Block("1 / 2 * 3").unwrap(), // (1 / 2) * 3
        e!(BinOp, (e!(BinOp, t!(Number, 1), BinOpCode::Div, t!(Number, 2))), BinOpCode::Mul, t!(Number, 3))
    );

    //assignment last
    assert_eq!(
        parse_Block("a = 1 * 2").unwrap(), // a = (1 * 2)
        e!(BinOp, t!(Id, "a".to_owned()), BinOpCode::Ass, e!(BinOp, t!(Number, 1), BinOpCode::Mul, t!(Number, 2)))
    );
}

#[test]
fn assignment() {
    assert_eq!(
        parse_Block("a = 4").unwrap(),
        e!(BinOp, t!(Id, "a".to_owned()), BinOpCode::Ass, t!(Number, 4))
    );
}

#[test]
#[should_panic]
fn assignment_non_variable() {
    assert_eq!(
        parse_Block("8 = 4").unwrap(),
        e!(BinOp, t!(Number, 8), BinOpCode::Ass, t!(Number, 4))
    );
}

#[test]
#[should_panic]
fn assignment_expression() {
    assert_eq!(
        parse_Block("2 * a = 4").unwrap(),
        t!(Null)
    );
}

