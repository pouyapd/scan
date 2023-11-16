use std::collections::HashMap;

// Use of "Newtype" pattern to define different types of indexes.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Var(pub usize);

pub type Integer = i32;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum Val {
    Boolean(bool),
    Integer(Integer),
}

pub type Eval = HashMap<Var, Val>;

#[derive(Debug)]
pub enum TypeErr {
    Mismatched,
    Uninitialized,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Const(Integer),
    Var(Var),
    Opposite(Box<Expr>),
    Sum(Box<Expr>, Box<Expr>),
    Mult(Box<Expr>, Box<Expr>),
}

impl Expr {
    pub fn eval(&self, eval: &Eval) -> Result<Integer, TypeErr> {
        match self {
            Expr::Const(int) => Ok(*int),
            Expr::Var(var) => {
                if let Val::Integer(val) = eval.get(var).ok_or(TypeErr::Uninitialized)? {
                    Ok(*val)
                } else {
                    Err(TypeErr::Mismatched)
                }
            }
            Expr::Opposite(expr) => expr.eval(eval).map(|val| -val),
            Expr::Sum(lhs, rhs) => Ok(lhs.eval(eval)? + rhs.eval(eval)?),
            Expr::Mult(lhs, rhs) => Ok(lhs.eval(eval)? * rhs.eval(eval)?),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Formula {
    And(Box<Formula>, Box<Formula>),
    Or(Box<Formula>, Box<Formula>),
    Implies(Box<Formula>, Box<Formula>),
    Not(Box<Formula>),
    Prop(Var),
    Equal(Expr, Expr),
    Less(Expr, Expr),
    LessEq(Expr, Expr),
    True,
    False,
}

impl Formula {
    pub fn eval(&self, eval: &Eval) -> Result<bool, TypeErr> {
        match self {
            Formula::And(lhs, rhs) => Ok(lhs.eval(eval)? && rhs.eval(eval)?),
            Formula::Or(lhs, rhs) => Ok(lhs.eval(eval)? || rhs.eval(eval)?),
            Formula::Implies(lhs, rhs) => Ok(!lhs.eval(eval)? || rhs.eval(eval)?),
            Formula::Not(subform) => Ok(!subform.eval(eval)?),
            Formula::Prop(var) => {
                let val = eval.get(var).ok_or(TypeErr::Uninitialized)?;
                if let Val::Boolean(prop) = val {
                    Ok(*prop)
                } else {
                    Err(TypeErr::Mismatched)
                }
            }
            Formula::Equal(lhs, rhs) => Ok(lhs.eval(eval)? == rhs.eval(eval)?),
            Formula::Less(lhs, rhs) => Ok(lhs.eval(eval)? < rhs.eval(eval)?),
            Formula::LessEq(lhs, rhs) => Ok(lhs.eval(eval)? <= rhs.eval(eval)?),
            Formula::True => Ok(true),
            Formula::False => Ok(false),
        }
    }
}
