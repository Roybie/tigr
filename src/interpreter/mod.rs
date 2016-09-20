use ast::*;
use std::collections::HashMap;

#[derive(Clone, Debug)]
struct Env(HashMap<String, Type>);

impl Env {
    fn new() -> Env {
        return Env(HashMap::new());
    }

    fn add(&mut self, id: String, value: Type) {
        let ref mut m = self.0;
        m.insert(id, value);
    }

    fn get(&self, id: String) -> Type {
        let ref m = self.0;
        match m.get(&id) {
            Some(ty) => ty.clone(),
            _ => Type::Null,
        }
    }
}

pub struct Eval {
    env: Vec<Env>,
}

impl Eval {
    pub fn new() -> Eval {
        Eval {
            env: vec!(Env::new())
        }
    }

    pub fn print(&self) {
        println!("Env:\n{:?}", self.env);
    }

    fn remove_scope(&mut self) {
        let old_scope = self.env.remove(0);
        for (var, val) in &old_scope.0 {
            if self.env[0].0.contains_key(var) {
                self.env[0].add(var.clone(), val.clone());
            }
        }
    }

    fn expr_to_bool(&mut self, e: Expr) -> Type {
        match self.eval(e) {
            Type::Bool(b) => Type::Bool(b),
            Type::Number(n) if n == 0 => Type::Bool(false),
            Type::String(ref s) if s == "" => Type::Bool(false),
            Type::Null => Type::Bool(false),
            _ => Type::Bool(true),
        }
    }

    pub fn eval(&mut self, expr: Expr) -> Type {
        match expr {
            Expr::Type(t) => {
                match t {
                    Type::Id(id) => {
                        self.env[0].get(id)
                    },
                    Type::Array(arr) => {
                        let mut new_vec = vec!();
                        for e in arr.iter() {
                            let evaluated = self.eval(*e.clone());
                            new_vec.push(Box::new(Expr::Type(evaluated)));
                        }
                        Type::Array(new_vec)
                    },
                    _ => t,
                }
            },
            Expr::Block(v, e) => {
                for exp in v.iter() {
                    self.eval(*exp.clone());
                }
                self.eval(*e)
            },
            Expr::UnOp(o, e) => {
                self.eval_unop(o, *e)
            },
            Expr::BinOp(e1, o, e2) => {
                self.eval_binop(*e1, o, *e2)
            },
            Expr::Scope(e) => {
                //create new scope
                let new_scope = self.env[0].clone();
                self.env.insert(0, new_scope);
                let t = self.eval(*e);
                // remove new scope
                self.remove_scope();
                t
            },
            Expr::If(condition, met_branch, else_branch) => {
                match self.expr_to_bool(*condition) {
                    Type::Bool(true) => self.eval(*met_branch),
                    _ => self.eval(*else_branch),
                }
            },
            Expr::For(for_args, for_scope) => {
                if let Expr::Args(ref for_args) = *for_args {
                    //create new scope
                    let new_scope = self.env[0].clone();
                    self.env.insert(0, new_scope);

                    let mut en = Expr::Type(Type::Null);
                    let mut it = Expr::Type(Type::Null);
                    let ra;
                    let mut range_from: i64 = 0;
                    let mut range_to: i64 = 0;
                    let mut range_step: i64 = 0;
                    let mut result = Type::Null;
                    match for_args.len() {
                        3 => {
                            en = *for_args[0].clone();
                            it = *for_args[1].clone();
                            ra = for_args[2].clone();
                        },
                        2 => {
                            it = *for_args[0].clone();
                            ra = for_args[1].clone();
                        },
                        1 => {
                            ra = for_args[0].clone();
                        },
                        _ => panic!("Invalid arguments to for")
                    };
                    if let Expr::Range(from, to, step) = *ra.clone() {
                        match (self.eval(*from), self.eval(*to), self.eval(*step)) {
                            (Type::Number(f), Type::Number(t), Type::Number(s)) => {
                                range_from = f;
                                range_to = t;
                                range_step = s;
                            },
                            _ => panic!("Range parameters must be numbers"),
                        };
                    }
                    //for loop!
                    let en = match en {
                        Expr::Type(Type::Id(id)) => id,
                        _ => "_".to_owned(),
                    };
                    let it = match it {
                        Expr::Type(Type::Id(id)) => id,
                        _ => "_".to_owned(),
                    };
                    let mut enumeration = 0;
                    while range_from < range_to {
                        if en != "_" {
                            self.env[0].add(en.clone(), Type::Number(enumeration));
                        }
                        if it != "_" {
                            self.env[0].add(it.clone(), Type::Number(range_from));
                        }
                        result = self.eval(*for_scope.clone());
                        enumeration += 1;
                        range_from += range_step;
                    }
                    //remove new scope
                    self.remove_scope();
                    result
                } else {
                    Type::Null
                }
            },
            Expr::ForA(for_args, for_scope) => {
                if let Expr::Args(ref for_args) = *for_args {
                    //create new scope
                    let new_scope = self.env[0].clone();
                    self.env.insert(0, new_scope);

                    let mut en = Expr::Type(Type::Null);
                    let mut it = Expr::Type(Type::Null);
                    let ra;
                    let mut range_from: i64 = 0;
                    let mut range_to: i64 = 0;
                    let mut range_step: i64 = 0;
                    let mut result: Vec<Box<Expr>> = vec!();
                    match for_args.len() {
                        3 => {
                            en = *for_args[0].clone();
                            it = *for_args[1].clone();
                            ra = for_args[2].clone();
                        },
                        2 => {
                            it = *for_args[0].clone();
                            ra = for_args[1].clone();
                        },
                        1 => {
                            ra = for_args[0].clone();
                        },
                        _ => panic!("Invalid arguments to for")
                    };
                    if let Expr::Range(from, to, step) = *ra.clone() {
                        match (self.eval(*from), self.eval(*to), self.eval(*step)) {
                            (Type::Number(f), Type::Number(t), Type::Number(s)) => {
                                range_from = f;
                                range_to = t;
                                range_step = s;
                            },
                            _ => panic!("Range parameters must be numbers"),
                        };
                    }
                    //for loop!
                    let en = match en {
                        Expr::Type(Type::Id(id)) => id,
                        _ => "_".to_owned(),
                    };
                    let it = match it {
                        Expr::Type(Type::Id(id)) => id,
                        _ => "_".to_owned(),
                    };
                    let mut enumeration = 0;
                    while range_from < range_to {
                        if en != "_" {
                            self.env[0].add(en.clone(), Type::Number(enumeration));
                        }
                        if it != "_" {
                            self.env[0].add(it.clone(), Type::Number(range_from));
                        }
                        result.push(Box::new(Expr::Type(self.eval(*for_scope.clone()))));
                        enumeration += 1;
                        range_from += range_step;
                    }
                    //remove new scope
                    self.remove_scope();
                    Type::Array(result)
                } else {
                    Type::Null
                }
            }
            _ => Type::Null,
        }
    }

    fn eval_unop(&mut self, o: UnOpCode, e: Expr) -> Type {
        match o {
            UnOpCode::Neg => {
                match self.eval(e) {
                    Type::Number(n) => Type::Number(-n),
                    _ => Type::Null,
                }
            },
            UnOpCode::Not => {
                match self.expr_to_bool(e) {
                    Type::Bool(b) => Type::Bool(!b),
                    _ => Type::Bool(false),
                }
            },
        }
    }

    fn eval_binop(&mut self, e1: Expr, o: BinOpCode, e2: Expr) -> Type {
        match o {
            BinOpCode::Ass => {
                //e1 must be an Id, from grammar rules
                let id = match e1 {
                    Expr::Type(Type::Id(id)) => id,
                    _ => panic!("Invalid assignment, LHS not an Id"),
                };
                let e2 = self.eval(e2);
                self.env[0].add(id, e2.clone());
                e2
            },
            BinOpCode::Mul => {
                match (self.eval(e1), self.eval(e2)) {
                    (Type::Number(n1), Type::Number(n2)) => Type::Number(n1 * n2),
                    _ => Type::Null,
                }
            },
            BinOpCode::Div => {
                match (self.eval(e1), self.eval(e2)) {
                    (Type::Number(n1), Type::Number(n2)) => Type::Number(n1 / n2),
                    _ => Type::Null,
                }
            },
            BinOpCode::Add => {
                match (self.eval(e1), self.eval(e2)) {
                    (Type::Number(n1), Type::Number(n2)) => Type::Number(n1 + n2),
                    _ => Type::Null,
                }
            },
            BinOpCode::Sub => {
                match (self.eval(e1), self.eval(e2)) {
                    (Type::Number(n1), Type::Number(n2)) => Type::Number(n1 - n2),
                    _ => Type::Null,
                }
            },
            BinOpCode::Equ => {
                match (self.eval(e1), self.eval(e2)) {
                    (Type::Number(n1), Type::Number(n2)) => Type::Bool(n1 == n2),
                    _ => Type::Null,
                }
            },
            BinOpCode::Neq => {
                match (self.eval(e1), self.eval(e2)) {
                    (Type::Number(n1), Type::Number(n2)) => Type::Bool(n1 != n2),
                    _ => Type::Null,
                }
            },
            BinOpCode::Lt => {
                match (self.eval(e1), self.eval(e2)) {
                    (Type::Number(n1), Type::Number(n2)) => Type::Bool(n1 < n2),
                    _ => Type::Null,
                }
            },
            BinOpCode::LEt => {
                match (self.eval(e1), self.eval(e2)) {
                    (Type::Number(n1), Type::Number(n2)) => Type::Bool(n1 <= n2),
                    _ => Type::Null,
                }
            },
            BinOpCode::Gt => {
                match (self.eval(e1), self.eval(e2)) {
                    (Type::Number(n1), Type::Number(n2)) => Type::Bool(n1 > n2),
                    _ => Type::Null,
                }
            },
            BinOpCode::GEt => {
                match (self.eval(e1), self.eval(e2)) {
                    (Type::Number(n1), Type::Number(n2)) => Type::Bool(n1 >= n2),
                    _ => Type::Null,
                }
            },
            //_ => Type::Null,
       }
    }
}
