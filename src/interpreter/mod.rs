use ast::*;
use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;

#[derive(Clone, Debug)]
struct Env{
    parent: Option<Rc<RefCell<Env>>>,
    values: HashMap<String, Type>,
}

impl Env {
    fn new_root() -> Rc<RefCell<Env>> {
        let env = Env { parent: None, values: HashMap::new() };
        Rc::new(RefCell::new(env))
    }

    fn new_child(parent: Rc<RefCell<Env>>) -> Rc<RefCell<Env>> {
        let env = Env { parent: Some(parent), values: HashMap::new() };
        Rc::new(RefCell::new(env))
    }

    fn define(&mut self, id: String, value: Type) {
        if self.values.contains_key(&id) {
            panic!("Duplicate define: {:?}", id)
        } else {
            self.values.insert(id, value);
        }
    }

    fn set(&mut self, id: String, value: Type) {
        if self.values.contains_key(&id) {
            self.values.insert(id, value);
        } else {
            match self.parent {
                Some(ref parent) => {
                    if parent.borrow_mut().set_if_present(id.clone(), value.clone()) == false {
                        self.values.insert(id, value);
                    }
                },
                None => { self.values.insert(id, value); },
            }
        }
    }

    fn set_if_present(&mut self, id: String, value: Type) -> bool {
        if self.values.contains_key(&id) {
            self.values.insert(id, value);
            return true;
        } else {
            match self.parent {
                Some(ref parent) => return parent.borrow_mut().set_if_present(id, value),
                None => return false,
            }
        }
    }

    fn get(&self, id: &String) -> Type {
        match self.values.get(id) {
            Some(val) => val.clone(),
            None => {
                match self.parent {
                    Some(ref parent) => parent.borrow().get(id),
                    None => Type::Null,
                }
            }
        }
    }
}

pub struct Eval {
    env: Rc<RefCell<Env>>,
}

impl Eval {
    pub fn new() -> Eval {
        Eval {
            env: Env::new_root()
        }
    }

    pub fn print(&self) {
        println!("Env:\n{:?}", self.env.borrow().values);
    }

    pub fn evaluate(&mut self, expr:Expr) -> Type {
        eval(expr, self.env.clone())
    }
}

fn expr_to_bool(e: Expr, env: Rc<RefCell<Env>>) -> Type {
    match eval(e, env.clone()) {
        Type::Bool(b) => Type::Bool(b),
        Type::Number(n) if n == 0 => Type::Bool(false),
        Type::Float(f) if f == 0.0 => Type::Bool(false),
        Type::String(ref s) if s == "" => Type::Bool(false),
        Type::Null => Type::Bool(false),
        _ => Type::Bool(true),
    }
}

fn eval(expr: Expr, env: Rc<RefCell<Env>>) -> Type {
    match expr {
        Expr::Type(t) => {
            match t {
                Type::Id(id) => {
                    env.borrow().get(&id)
                },
                Type::Array(arr) => {
                    let mut new_vec = vec!();
                    for e in arr.iter() {
                        let evaluated = eval(*e.clone(), env.clone());
                        new_vec.push(Box::new(Expr::Type(evaluated)));
                    }
                    Type::Array(new_vec)
                },
                Type::Break(b) => Type::Break(Box::new(Expr::Type(eval(*b, env.clone())))),
                _ => t,
            }
        },
        Expr::Index(a, i) => {
            let index = match eval(*i, env.clone()) {
                Type::Number(n) => n,
                _ => panic!("Index must be integer"),
            };
            match eval(*a, env.clone()) {
                Type::Array(arr) => {
                    match arr.get(index as usize) {
                        Some(v) => eval(*v.clone(), env.clone()),
                        None => Type::Null,
                    }
                },
                _ => panic!("Cannot index non-array type"),
            }
        },
        Expr::Block(v, e) => {
            for exp in v.iter() {
                eval(*exp.clone(), env.clone());
            }
            eval(*e, env.clone())
        },
        Expr::UnOp(o, e) => {
            eval_unop(o, *e, env.clone())
        },
        Expr::BinOp(e1, o, e2) => {
            eval_binop(*e1, o, *e2, env.clone())
        },
        Expr::Scope(e) => {
            //create new scope
            let env = Env::new_child(env.clone());
            let t = eval(*e, env.clone());
            t
        },
        Expr::If(condition, met_branch, else_branch) => {
            match expr_to_bool(*condition, env.clone()) {
                Type::Bool(true) => eval(*met_branch, env.clone()),
                _ => eval(*else_branch, env.clone()),
            }
        },
        Expr::While(condition, scope) => {
            let mut result = Type::Null;
            while match expr_to_bool(*condition.clone(), env.clone()) {
                Type::Bool(true) => true,
                _ => false,
            } {
                result = eval(*scope.clone(), env.clone());
                match result {
                    Type::Break(b) => {
                        result = eval(*b, env.clone());
                        break
                    },
                    _ => (),
                }
            }
            result
        },
        Expr::WhileA(condition, scope) => {
            let mut result: Vec<Box<Expr>> = vec!();
            while match expr_to_bool(*condition.clone(), env.clone()) {
                Type::Bool(true) => true,
                _ => false,
            } {
                let mut temp_result = eval(*scope.clone(), env.clone());
                match temp_result {
                    Type::Break(b) => {
                        temp_result = eval(*b, env.clone());
                        result.push(Box::new(Expr::Type(temp_result)));
                        break
                    },
                    _ => (),
                }
                result.push(Box::new(Expr::Type(temp_result)));
            }
            Type::Array(result)
        },
        Expr::For(for_args, for_scope) => {
            if let Expr::Args(ref for_args) = *for_args {
                let new_env = Env::new_child(env.clone());

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
                    match (eval(*from, new_env.clone()), eval(*to, new_env.clone()), eval(*step, new_env.clone())) {
                        (Type::Number(f), Type::Number(t), Type::Number(s)) => {
                            range_from = f;
                            range_to = t;
                            range_step = s;
                            if range_from > range_to && range_step > 0 {
                                range_step *= -1;
                            } else if range_from < range_to && range_step < 0 {
                                range_step *= -1;
                            }
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
                if en != "_" {
                    new_env.borrow_mut().define(en.clone(), Type::Number(0));
                }
                if it != "_" {
                    new_env.borrow_mut().define(it.clone(), Type::Number(0));
                }
                let mut enumeration = 0;
                while (range_from - range_to).abs() >= range_step.abs() {
                    if en != "_" {
                        new_env.borrow_mut().set(en.clone(), Type::Number(enumeration));
                    }
                    if it != "_" {
                        new_env.borrow_mut().set(it.clone(), Type::Number(range_from));
                    }
                    result = eval(*for_scope.clone(), new_env.clone());
                    match result {
                        Type::Break(b) => {
                            result = eval(*b, new_env.clone());
                            break
                        },
                        _ => (),
                    }
                    enumeration += 1;
                    range_from += range_step;
                }
                result
            } else {
                Type::Null
            }
        },
        Expr::ForA(for_args, for_scope) => {
            if let Expr::Args(ref for_args) = *for_args {
                let new_env = Env::new_child(env.clone());

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
                    match (eval(*from, new_env.clone()), eval(*to, new_env.clone()), eval(*step, new_env.clone())) {
                        (Type::Number(f), Type::Number(t), Type::Number(s)) => {
                            range_from = f;
                            range_to = t;
                            range_step = s;
                            if range_from > range_to && range_step > 0 {
                                range_step *= -1;
                            } else if range_from < range_to && range_step < 0 {
                                range_step *= -1;
                            }
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
                if en != "_" {
                    new_env.borrow_mut().define(en.clone(), Type::Number(0));
                }
                if it != "_" {
                    new_env.borrow_mut().define(it.clone(), Type::Number(0));
                }
                let mut enumeration = 0;
                while (range_from - range_to).abs() >= range_step.abs() {
                    if en != "_" {
                        new_env.borrow_mut().set(en.clone(), Type::Number(enumeration));
                    }
                    if it != "_" {
                        new_env.borrow_mut().set(it.clone(), Type::Number(range_from));
                    }
                    let mut temp_result = eval(*for_scope.clone(), new_env.clone());
                    match temp_result {
                        Type::Break(b) => {
                            temp_result = eval(*b, new_env.clone());
                            result.push(Box::new(Expr::Type(temp_result)));
                            break
                        },
                        _ => (),
                    }
                    result.push(Box::new(Expr::Type(temp_result)));
                    enumeration += 1;
                    range_from += range_step;
                }
                Type::Array(result)
            } else {
                Type::Null
            }
        },
        Expr::FuncCall(func, args) => {
            match eval(*func, env.clone()) {
                Type::Function (a,s) => {
                    let new_env = Env::new_child(env.clone());

                    let mut eval_args: Vec<Type> = vec!();
                    if let Expr::Args(args) = *args.clone() {
                        for arg in args.iter() {
                            eval_args.push(eval(*arg.clone(), env.clone()));
                        }
                    }
                    //bind to function definition args

                    if let Expr::Args(args) = *a {
                        for arg in args.iter() {
                            match **arg {
                                Expr::Type(Type::Id(ref id)) => new_env.borrow_mut().define(id.clone(), if eval_args.len() > 0 {eval_args.remove(0)} else {Type::Null}),
                                _ => (),
                            }
                        }
                    }
                    return eval(*s, new_env.clone());
                },
                e => panic!("Can't call {:?} as a function", e)
            }
        },
        _ => Type::Null,
    }
}

fn eval_unop(o: UnOpCode, e: Expr, env: Rc<RefCell<Env>>) -> Type {
    match o {
        UnOpCode::Neg => {
            match eval(e, env.clone()) {
                Type::Number(n) => Type::Number(-n),
                Type::Float(f) => Type::Float(-f),
                _ => Type::Null,
            }
        },
        UnOpCode::Not => {
            match expr_to_bool(e, env.clone()) {
                Type::Bool(b) => Type::Bool(!b),
                _ => Type::Bool(false),
            }
        },
        UnOpCode::Len => {
            match eval(e, env.clone()) {
                Type::Array(a) => Type::Number(a.len() as i64),
                _ => Type::Number(0),
            }
        },
    }
}

fn eval_binop(e1: Expr, o: BinOpCode, e2: Expr, env: Rc<RefCell<Env>>) -> Type {
    match o {
        BinOpCode::Ass => {
            match e1 {
                Expr::Type(Type::Id(id)) => {
                    let e2 = eval(e2, env.clone());
                    env.borrow_mut().set(id, e2.clone());
                    e2
                },
                Expr::Index(a, i) => {
                    let index = match eval(*i, env.clone()) {
                        Type::Number(n) => n,
                        _ => panic!("Index must be integer"),
                    };
                    let id = match *a {
                        Expr::Type(Type::Id(ref id)) => id.clone(),
                        _ => "_".to_owned(),
                    };
                    let e2 = eval(e2, env.clone());
                    match eval(*a, env.clone()) {
                        Type::Array(mut arr) => {
                            *arr[index as usize] = Expr::Type(e2.clone());
                            if id != "_" {
                                env.borrow_mut().set(id, Type::Array(arr));
                            };
                        },
                        _ => panic!("Cannot index non-array type {:?}", id),
                    };
                    e2
                },
                _ => panic!("Invalid assignment, LHS not a valid assignee"),
            }
        },
        BinOpCode::AddEq => {
            match e1 {
                Expr::Type(Type::Id(id)) => {
                    let current_val = env.borrow().get(&id);
                    let new_value = eval(Expr::BinOp(Box::new(Expr::Type(current_val)),BinOpCode::Add, Box::new(e2.clone())), env.clone());
                    env.borrow_mut().set(id, new_value.clone());
                    new_value
                },
                Expr::Index(a, i) => {
                    let index = match eval(*i, env.clone()) {
                        Type::Number(n) => n,
                        _ => panic!("Index must be integer"),
                    };
                    let id = match *a {
                        Expr::Type(Type::Id(ref id)) => id.clone(),
                        _ => panic!("Invalid assignment"),
                    };
                    match eval(*a, env.clone()) {
                        Type::Array(mut arr) => {
                            let current_val = *arr[index as usize].clone();
                            let new_value = eval(Expr::BinOp(Box::new(current_val),BinOpCode::Add, Box::new(e2.clone())), env.clone());
                            *arr[index as usize] = Expr::Type(new_value.clone());
                            env.borrow_mut().set(id, Type::Array(arr));
                            new_value
                        },
                        _ => panic!("Cannot index non-array type {:?}", id),
                    }
                },
                _ => panic!("Invalid assignment, LHS not a valid assignee"),
            }
        },
        BinOpCode::SubEq => {
            match e1 {
                Expr::Type(Type::Id(id)) => {
                    let current_val = env.borrow().get(&id);
                    let new_value = eval(Expr::BinOp(Box::new(Expr::Type(current_val)),BinOpCode::Sub, Box::new(e2.clone())), env.clone());
                    env.borrow_mut().set(id, new_value.clone());
                    new_value
                },
                Expr::Index(a, i) => {
                    let index = match eval(*i, env.clone()) {
                        Type::Number(n) => n,
                        _ => panic!("Index must be integer"),
                    };
                    let id = match *a {
                        Expr::Type(Type::Id(ref id)) => id.clone(),
                        _ => panic!("Invalid assignment"),
                    };
                    match eval(*a, env.clone()) {
                        Type::Array(mut arr) => {
                            let current_val = *arr[index as usize].clone();
                            let new_value = eval(Expr::BinOp(Box::new(current_val),BinOpCode::Sub, Box::new(e2.clone())), env.clone());
                            *arr[index as usize] = Expr::Type(new_value.clone());
                            env.borrow_mut().set(id, Type::Array(arr));
                            new_value
                        },
                        _ => panic!("Cannot index non-array type {:?}", id),
                    }
                },
                _ => panic!("Invalid assignment, LHS not a valid assignee"),
            }
        },
        BinOpCode::MulEq => {
            match e1 {
                Expr::Type(Type::Id(id)) => {
                    let current_val = env.borrow().get(&id);
                    let new_value = eval(Expr::BinOp(Box::new(Expr::Type(current_val)),BinOpCode::Mul, Box::new(e2.clone())), env.clone());
                    env.borrow_mut().set(id, new_value.clone());
                    new_value
                },
                Expr::Index(a, i) => {
                    let index = match eval(*i, env.clone()) {
                        Type::Number(n) => n,
                        _ => panic!("Index must be integer"),
                    };
                    let id = match *a {
                        Expr::Type(Type::Id(ref id)) => id.clone(),
                        _ => panic!("Invalid assignment"),
                    };
                    match eval(*a, env.clone()) {
                        Type::Array(mut arr) => {
                            let current_val = *arr[index as usize].clone();
                            let new_value = eval(Expr::BinOp(Box::new(current_val),BinOpCode::Mul, Box::new(e2.clone())), env.clone());
                            *arr[index as usize] = Expr::Type(new_value.clone());
                            env.borrow_mut().set(id, Type::Array(arr));
                            new_value
                        },
                        _ => panic!("Cannot index non-array type {:?}", id),
                    }
                },
                _ => panic!("Invalid assignment, LHS not a valid assignee"),
            }
        },
        BinOpCode::DivEq => {
            match e1 {
                Expr::Type(Type::Id(id)) => {
                    let current_val = env.borrow().get(&id);
                    let new_value = eval(Expr::BinOp(Box::new(Expr::Type(current_val)),BinOpCode::Div, Box::new(e2.clone())), env.clone());
                    env.borrow_mut().set(id, new_value.clone());
                    new_value
                },
                Expr::Index(a, i) => {
                    let index = match eval(*i, env.clone()) {
                        Type::Number(n) => n,
                        _ => panic!("Index must be integer"),
                    };
                    let id = match *a {
                        Expr::Type(Type::Id(ref id)) => id.clone(),
                        _ => panic!("Invalid assignment"),
                    };
                    match eval(*a, env.clone()) {
                        Type::Array(mut arr) => {
                            let current_val = *arr[index as usize].clone();
                            let new_value = eval(Expr::BinOp(Box::new(current_val),BinOpCode::Div, Box::new(e2.clone())), env.clone());
                            *arr[index as usize] = Expr::Type(new_value.clone());
                            env.borrow_mut().set(id, Type::Array(arr));
                            new_value
                        },
                        _ => panic!("Cannot index non-array type {:?}", id),
                    }
                },
                _ => panic!("Invalid assignment, LHS not a valid assignee"),
            }
        },
        BinOpCode::ModEq => {
            match e1 {
                Expr::Type(Type::Id(id)) => {
                    let current_val = env.borrow().get(&id);
                    let new_value = eval(Expr::BinOp(Box::new(Expr::Type(current_val)),BinOpCode::Mod, Box::new(e2.clone())), env.clone());
                    env.borrow_mut().set(id, new_value.clone());
                    new_value
                },
                Expr::Index(a, i) => {
                    let index = match eval(*i, env.clone()) {
                        Type::Number(n) => n,
                        _ => panic!("Index must be integer"),
                    };
                    let id = match *a {
                        Expr::Type(Type::Id(ref id)) => id.clone(),
                        _ => panic!("Invalid assignment"),
                    };
                    match eval(*a, env.clone()) {
                        Type::Array(mut arr) => {
                            let current_val = *arr[index as usize].clone();
                            let new_value = eval(Expr::BinOp(Box::new(current_val),BinOpCode::Mod, Box::new(e2.clone())), env.clone());
                            *arr[index as usize] = Expr::Type(new_value.clone());
                            env.borrow_mut().set(id, Type::Array(arr));
                            new_value
                        },
                        _ => panic!("Cannot index non-array type {:?}", id),
                    }
                },
                _ => panic!("Invalid assignment, LHS not a valid assignee"),
            }
        },
        BinOpCode::Mul => {
            match (eval(e1, env.clone()), eval(e2, env.clone())) {
                (Type::Number(n1), Type::Number(n2)) => Type::Number(n1 * n2),
                (Type::Number(n), Type::Float(f)) => Type::Float(n as f64 * f),
                (Type::Float(f), Type::Number(n)) => Type::Float(f * n as f64),
                (Type::Float(f1), Type::Float(f2)) => Type::Float(f1 * f2),
                _ => Type::Null,
            }
        },
        BinOpCode::Div => {
            match (eval(e1, env.clone()), eval(e2, env.clone())) {
                (Type::Number(n1), Type::Number(n2)) => {
                    if n1 % n2 == 0 {
                        Type::Number(n1 / n2)
                    } else {
                        Type::Float(n1 as f64 / n2 as f64)
                    }
                },
                (Type::Number(n), Type::Float(f)) => Type::Float(n as f64 / f),
                (Type::Float(f), Type::Number(n)) => Type::Float(f / n as f64),
                (Type::Float(f1), Type::Float(f2)) => Type::Float(f1 / f2),
                _ => Type::Null,
            }
        },
        BinOpCode::Add => {
            match (eval(e1, env.clone()), eval(e2, env.clone())) {
                (Type::Number(n1), Type::Number(n2)) => Type::Number(n1 + n2),
                (Type::Number(n), Type::Float(f)) => Type::Float(n as f64 + f),
                (Type::Float(f), Type::Number(n)) => Type::Float(f + n as f64),
                (Type::Float(f1), Type::Float(f2)) => Type::Float(f1 + f2),
                (Type::Array(ref mut a), Type::Array(ref mut to_add)) => {
                    a.append(to_add);
                    Type::Array(a.clone())
                },
                (Type::Array(mut a), to_add) => {
                    a.push(Box::new(Expr::Type(to_add)));
                    Type::Array(a.clone())
                },
                _ => Type::Null,
            }
        },
        BinOpCode::Sub => {
            match (eval(e1, env.clone()), eval(e2, env.clone())) {
                (Type::Number(n1), Type::Number(n2)) => Type::Number(n1 - n2),
                (Type::Number(n), Type::Float(f)) => Type::Float(n as f64 - f),
                (Type::Float(f), Type::Number(n)) => Type::Float(f - n as f64),
                (Type::Float(f1), Type::Float(f2)) => Type::Float(f1 - f2),
                _ => Type::Null,
            }
        },
        BinOpCode::Mod => {
            match (eval(e1, env.clone()), eval(e2, env.clone())) {
                (Type::Number(n1), Type::Number(n2)) => Type::Number(n1 % n2),
                (Type::Number(n), Type::Float(f)) => Type::Float(n as f64 % f),
                (Type::Float(f), Type::Number(n)) => Type::Float(f % n as f64),
                (Type::Float(f1), Type::Float(f2)) => Type::Float(f1 % f2),
                _ => Type::Null,
            }
        },
        BinOpCode::And => Type::Bool(match (expr_to_bool(e1, env.clone()), expr_to_bool(e2, env.clone())) {
            (Type::Bool(b1), Type::Bool(b2)) => b1 && b2,
            _ => false,
        }),
        BinOpCode::Or => Type::Bool(match (expr_to_bool(e1, env.clone()), expr_to_bool(e2, env.clone())) {
            (Type::Bool(b1), Type::Bool(b2)) => b1 || b2,
            _ => false,
        }),
        BinOpCode::Equ => {
            match (eval(e1, env.clone()), eval(e2, env.clone())) {
                (Type::Number(n1), Type::Number(n2)) => Type::Bool(n1 == n2),
                (Type::Number(n), Type::Float(f)) | (Type::Float(f), Type::Number(n)) => Type::Bool(f == n as f64),
                (Type::Float(f1), Type::Float(f2)) => Type::Bool(f1 == f2),
                _ => Type::Null,
            }
        },
        BinOpCode::Neq => {
            match (eval(e1, env.clone()), eval(e2, env.clone())) {
                (Type::Number(n1), Type::Number(n2)) => Type::Bool(n1 != n2),
                (Type::Number(n), Type::Float(f)) | (Type::Float(f), Type::Number(n)) => Type::Bool(f != n as f64),
                (Type::Float(f1), Type::Float(f2)) => Type::Bool(f1 != f2),
                _ => Type::Null,
            }
        },
        BinOpCode::Lt => {
            match (eval(e1, env.clone()), eval(e2, env.clone())) {
                (Type::Number(n1), Type::Number(n2)) => Type::Bool(n1 < n2),
                (Type::Number(n), Type::Float(f)) | (Type::Float(f), Type::Number(n)) => Type::Bool(f < n as f64),
                (Type::Float(f1), Type::Float(f2)) => Type::Bool(f1 < f2),
                _ => Type::Null,
            }
        },
        BinOpCode::LEt => {
            match (eval(e1, env.clone()), eval(e2, env.clone())) {
                (Type::Number(n1), Type::Number(n2)) => Type::Bool(n1 <= n2),
                (Type::Number(n), Type::Float(f)) | (Type::Float(f), Type::Number(n)) => Type::Bool(f <= n as f64),
                (Type::Float(f1), Type::Float(f2)) => Type::Bool(f1 <= f2),
                _ => Type::Null,
            }
        },
        BinOpCode::Gt => {
            match (eval(e1, env.clone()), eval(e2, env.clone())) {
                (Type::Number(n1), Type::Number(n2)) => Type::Bool(n1 > n2),
                (Type::Number(n), Type::Float(f)) | (Type::Float(f), Type::Number(n)) => Type::Bool(f > n as f64),
                (Type::Float(f1), Type::Float(f2)) => Type::Bool(f1 > f2),
                _ => Type::Null,
            }
        },
        BinOpCode::GEt => {
            match (eval(e1, env.clone()), eval(e2, env.clone())) {
                (Type::Number(n1), Type::Number(n2)) => Type::Bool(n1 >= n2),
                (Type::Number(n), Type::Float(f)) | (Type::Float(f), Type::Number(n)) => Type::Bool(f >= n as f64),
                (Type::Float(f1), Type::Float(f2)) => Type::Bool(f1 >= f2),
                _ => Type::Null,
            }
        },
        //_ => Type::Null,
    }
}

