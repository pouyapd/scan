#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum Type {
    Unit,
    Boolean,
    Integer,
    Product(Vec<Type>),
}

impl Type {
    pub fn default_value(&self) -> Val {
        match self {
            Type::Boolean => Val::Boolean(false),
            Type::Integer => Val::Integer(0),
            Type::Unit => Val::Unit,
            Type::Product(tuple) => {
                Val::Tuple(Vec::from_iter(tuple.iter().map(Self::default_value)))
            }
        }
    }
}

pub type Integer = i32;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum Val {
    Unit,
    Boolean(bool),
    Integer(Integer),
    Tuple(Vec<Val>),
}

impl Val {
    pub fn r#type(&self) -> Type {
        match self {
            Val::Unit => Type::Unit,
            Val::Boolean(_) => Type::Boolean,
            Val::Integer(_) => Type::Integer,
            Val::Tuple(comps) => Type::Product(comps.iter().map(Val::r#type).collect()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Expression<V>
where
    V: Clone + PartialEq + Eq,
{
    // General expressions
    Const(Val),
    Var(V),
    Tuple(Vec<Expression<V>>),
    Component(usize, Box<Expression<V>>),
    // Logical operators
    And(Vec<Expression<V>>),
    Or(Vec<Expression<V>>),
    Implies(Box<(Expression<V>, Expression<V>)>),
    Not(Box<Expression<V>>),
    // Arithmetic operators
    Opposite(Box<Expression<V>>),
    Sum(Vec<Expression<V>>),
    Mult(Vec<Expression<V>>),
    Equal(Box<(Expression<V>, Expression<V>)>),
    Greater(Box<(Expression<V>, Expression<V>)>),
    GreaterEq(Box<(Expression<V>, Expression<V>)>),
    Less(Box<(Expression<V>, Expression<V>)>),
    LessEq(Box<(Expression<V>, Expression<V>)>),
}
