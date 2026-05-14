use ast::*;
use parser::*;
use lexer::Lexer;

macro_rules! e {
    ($t:ident, $($e:expr),+) => (Box::new(Expr::$t($($e),+)));
}

macro_rules! t {
    ($t:ident) => (Box::new(Expr::Type(Type::$t)));
    ($t:ident, $e:expr) => (Box::new(Expr::Type(Type::$t($e))));
}

//#[test]
//fn precidence_unary() {
    ////unary before anything
    //let src = "1 - -2";
    //assert_eq!(
        //parse_Block(src, scan(src)).unwrap(),
        //e!(BinOp, t!(Number, 1), BinOpCode::Sub, e!(UnOp, UnOpCode::Neg, t!(Number, 2)))
    //);

    //let src = "a = !true";
    //assert_eq!(
        //parse_Block(src, scan(src)).unwrap(),
        //e!(BinOp, t!(Id, "a".to_owned()), BinOpCode::Ass, e!(UnOp, UnOpCode::Not, t!(Bool, true)))
    //);
//}

//#[test]
//fn precidence_binary() {
    //// Mul/div before add/sub
    //let src = "1 + 2 * 3";
    //assert_eq!(
        //parse_Block(src, scan(src)).unwrap(), // 1 + (2 * 3)
        //e!(BinOp, t!(Number, 1), BinOpCode::Add, e!(BinOp, t!(Number, 2), BinOpCode::Mul, t!(Number, 3)))
    //);
    //let src = "1 - 2 / 3";
    //assert_eq!(
        //parse_Block(src, scan(src)).unwrap(), // 1 - (2 / 3)
        //e!(BinOp, t!(Number, 1), BinOpCode::Sub, e!(BinOp, t!(Number, 2), BinOpCode::Div, t!(Number, 3)))
    //);

    ////Add sub together, left associative
    //let src = "1 - 2 + 3";
    //assert_eq!(
        //parse_Block(src, scan(src)).unwrap(), // (1 - 2) + 3
        //e!(BinOp, (e!(BinOp, t!(Number, 1), BinOpCode::Sub, t!(Number, 2))), BinOpCode::Add, t!(Number, 3))
    //);

    ////mul div together, left associative
    //let src = "1 / 2 * 3";
    //assert_eq!(
        //parse_Block(src, scan(src)).unwrap(), // (1 / 2) * 3
        //e!(BinOp, (e!(BinOp, t!(Number, 1), BinOpCode::Div, t!(Number, 2))), BinOpCode::Mul, t!(Number, 3))
    //);

    ////assignment last
    //let src = "a = 1 * 2";
    //assert_eq!(
        //parse_Block(src, scan(src)).unwrap(), // a = (1 * 2)
        //e!(BinOp, t!(Id, "a".to_owned()), BinOpCode::Ass, e!(BinOp, t!(Number, 1), BinOpCode::Mul, t!(Number, 2)))
    //);
//}

//#[test]
//fn assignment() {
    //let src = "a = 4";
    //assert_eq!(
        //parse_Block(src, scan(src)).unwrap(),
        //e!(BinOp, t!(Id, "a".to_owned()), BinOpCode::Ass, t!(Number, 4))
    //);
//}

//#[test]
//#[should_panic]
//fn assignment_non_variable() {
    //let src = "8 = 4";
    //assert_eq!(
        //parse_Block(src, scan(src)).unwrap(),
        //e!(BinOp, t!(Number, 8), BinOpCode::Ass, t!(Number, 4))
    //);
//}

//#[test]
//#[should_panic]
//fn assignment_expression() {
    //let src = "2 * a = 4";
    //assert_eq!(
        //parse_Block(src, scan(src)).unwrap(),
        //t!(Null)
    //);
//}

#[test]
fn bools() {
    let src = "true";
    let lexer = Lexer::new(&src);
    match parse_Block(lexer) {
        Ok(s) => assert_eq!(s, t!(Bool, true)),
        Err(e) => panic!("{:?}", e),
    }
}
