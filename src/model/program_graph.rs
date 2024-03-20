// TODO: use fast hasher
use std::{collections::HashMap, error::Error, fmt, rc::Rc};

// Use of "Newtype" pattern to define different types of indexes.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Location(usize);

// Use of "Newtype" pattern to define different types of indexes.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Action(usize);

// Use of "Newtype" pattern to define different types of indexes.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Var(usize);

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum VarType {
    Unit,
    Boolean,
    Integer,
}

pub type Integer = i32;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum Val {
    Unit,
    Boolean(bool),
    Integer(Integer),
}

#[derive(Debug, Clone)]
pub enum IntExpr {
    Const(Integer),
    Var(Var),
    Opposite(Box<IntExpr>),
    Sum(Box<IntExpr>, Box<IntExpr>),
    Mult(Box<IntExpr>, Box<IntExpr>),
}

#[derive(Debug, Clone)]
pub enum Formula {
    And(Box<Formula>, Box<Formula>),
    Or(Box<Formula>, Box<Formula>),
    Implies(Box<Formula>, Box<Formula>),
    Not(Box<Formula>),
    Prop(Var),
    Equal(IntExpr, IntExpr),
    Less(IntExpr, IntExpr),
    LessEq(IntExpr, IntExpr),
    True,
    False,
}

#[derive(Debug, Clone)]
pub enum Expression {
    Unit,
    Boolean(Formula),
    Integer(IntExpr),
}

impl From<&Expression> for VarType {
    fn from(value: &Expression) -> Self {
        match value {
            Expression::Boolean(_) => VarType::Boolean,
            Expression::Integer(_) => VarType::Integer,
            Expression::Unit => VarType::Unit,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum PgError {
    MissingAction(Action),
    MissingLocation(Location),
    Mismatched,
    NonExistingVar(Var),
    NoTransition,
    UnsatisfiedGuard,
}

impl fmt::Display for PgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PgError::MissingAction(action) => write!(
                f,
                "action {:?} does not belong to this program graph",
                action
            ),
            PgError::MissingLocation(location) => write!(
                f,
                "location {:?} does not belong to this program graph",
                location
            ),
            PgError::Mismatched => write!(f, "type mismatch"),
            PgError::NonExistingVar(var) => {
                write!(f, "var {:?} does not belong to this program graph", var)
            }
            PgError::NoTransition => write!(f, "There is no such transition"),
            PgError::UnsatisfiedGuard => write!(f, "The guard has not been satisfied"),
        }
    }
}

impl Error for PgError {}

#[derive(Debug, Clone)]
pub struct ProgramGraphBuilder {
    // Effects are indexed by actions
    effects: Vec<Vec<(Var, Expression)>>,
    // Transitions are indexed by locations
    // We can assume there is at most one condition by logical disjunction
    transitions: Vec<HashMap<(Action, Location), Formula>>,
    vars: Vec<VarType>,
}

impl Default for ProgramGraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgramGraphBuilder {
    const INITIAL_LOCATION: Location = Location(0);

    pub fn new() -> Self {
        let mut pgb = Self {
            effects: Vec::new(),
            transitions: Vec::new(),
            vars: Vec::new(),
        };
        // Create an initial location and make sure it is equal to the constant `Self::INITIAL_LOCATION`
        // This is the simplest way to make sure the state of the builder is always consistent
        let initial_location = pgb.new_location();
        assert_eq!(initial_location, Self::INITIAL_LOCATION);
        pgb
    }

    pub fn var_type(&self, var: Var) -> Result<VarType, PgError> {
        self.vars
            .get(var.0)
            .ok_or(PgError::NonExistingVar(var))
            .cloned()
    }

    pub fn new_var(&mut self, var_type: VarType) -> Var {
        let idx = self.vars.len();
        self.vars.push(var_type);
        Var(idx)
    }

    pub fn new_action(&mut self) -> Action {
        // Actions are indexed progressively
        let idx = self.effects.len();
        self.effects.push(Vec::new());
        Action(idx)
    }

    pub fn add_effect(
        &mut self,
        action: Action,
        var: Var,
        effect: Expression,
    ) -> Result<(), PgError> {
        match self.vars.get(var.0).ok_or(PgError::NonExistingVar(var))? {
            VarType::Boolean if !matches!(effect, Expression::Boolean(_)) => {
                Err(PgError::Mismatched)
            }
            VarType::Integer if !matches!(effect, Expression::Integer(_)) => {
                Err(PgError::Mismatched)
            }
            _ => self
                .effects
                .get_mut(action.0)
                .ok_or(PgError::MissingAction(action))
                .map(|effects| effects.push((var, effect))),
        }
    }

    pub fn new_location(&mut self) -> Location {
        // Locations are indexed progressively
        let idx = self.transitions.len();
        self.transitions.push(HashMap::new());
        Location(idx)
    }

    pub fn add_transition(
        &mut self,
        pre: Location,
        action: Action,
        post: Location,
        guard: Formula,
    ) -> Result<(), PgError> {
        self.typecheck(&guard)?;
        // Check 'pre' and 'post' locations exists
        if self.transitions.len() <= pre.0 {
            return Err(PgError::MissingLocation(pre));
        }
        if self.transitions.len() <= post.0 {
            return Err(PgError::MissingLocation(post));
        }
        // Check 'action' exists
        if self.effects.len() <= action.0 {
            return Err(PgError::MissingAction(action));
        }
        let _ = self
            .transitions
            .get_mut(pre.0)
            .expect("location existance already checked")
            .entry((action, post))
            .and_modify(|previous_guard| {
                *previous_guard =
                    Formula::Or(Box::new(previous_guard.clone()), Box::new(guard.clone()));
            })
            .or_insert(guard);
        Ok(())
    }

    fn typecheck(&self, formula: &Formula) -> Result<(), PgError> {
        match formula {
            Formula::Not(subf) => self.typecheck(subf),
            Formula::And(lhs, rhs) | Formula::Or(lhs, rhs) | Formula::Implies(lhs, rhs) => {
                self.typecheck(lhs).and_then(|()| self.typecheck(rhs))
            }
            Formula::Prop(Var(idx)) => {
                let var = self
                    .vars
                    .get(*idx)
                    .ok_or(PgError::NonExistingVar(Var(*idx)))?;
                if matches!(var, VarType::Boolean) {
                    Ok(())
                } else {
                    Err(PgError::Mismatched)
                }
            }
            Formula::Equal(lhs, rhs) | Formula::Less(lhs, rhs) | Formula::LessEq(lhs, rhs) => self
                .typecheck_expr(lhs)
                .and_then(|()| self.typecheck_expr(rhs)),
            _ => Ok(()),
        }
    }

    fn typecheck_expr(&self, expr: &IntExpr) -> Result<(), PgError> {
        match expr {
            IntExpr::Const(_) => Ok(()),
            IntExpr::Var(Var(idx)) => {
                let var = self
                    .vars
                    .get(*idx)
                    .ok_or(PgError::NonExistingVar(Var(*idx)))?;
                if matches!(var, VarType::Integer) {
                    Ok(())
                } else {
                    Err(PgError::Mismatched)
                }
            }
            IntExpr::Opposite(expr) => self.typecheck_expr(expr),
            IntExpr::Sum(lhs, rhs) | IntExpr::Mult(lhs, rhs) => self
                .typecheck_expr(lhs)
                .and_then(|()| self.typecheck_expr(rhs)),
        }
    }

    pub fn build(mut self) -> ProgramGraph {
        // Since vectors of effects and transitions will become unmutable,
        // they should be shrunk to take as little space as possible
        self.effects.iter_mut().for_each(Vec::shrink_to_fit);
        self.effects.shrink_to_fit();
        self.transitions.shrink_to_fit();
        // Vars are not going to be unmutable,
        // but their number will be constant anyway
        self.vars.shrink_to_fit();
        // Build program graph
        ProgramGraph {
            current_location: Self::INITIAL_LOCATION,
            vars: self
                .vars
                .iter()
                .map(|var_type| match var_type {
                    VarType::Boolean => Val::Boolean(false),
                    VarType::Integer => Val::Integer(0),
                    VarType::Unit => Val::Unit,
                })
                .collect(),
            effects: Rc::new(self.effects),
            transitions: Rc::new(self.transitions),
        }
    }

    pub fn initial_location(&self) -> Location {
        Self::INITIAL_LOCATION
    }
}

#[derive(Debug, Clone)]
pub struct ProgramGraph {
    current_location: Location,
    vars: Vec<Val>,
    // TODO: use SmallVec optimization
    effects: Rc<Vec<Vec<(Var, Expression)>>>,
    transitions: Rc<Vec<HashMap<(Action, Location), Formula>>>,
}

impl ProgramGraph {
    pub fn possible_transitions(&self) -> Vec<(Action, Location)> {
        self.transitions
            .get(self.current_location.0)
            .unwrap_or(&HashMap::new())
            .iter()
            .filter_map(|((action, post), guard)| {
                if self.eval_formula(guard) {
                    Some((*action, *post))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    }

    pub fn transition(&mut self, action: Action, post_state: Location) -> Result<(), PgError> {
        let guard = self
            .transitions
            .get(self.current_location.0)
            .expect("location must exist")
            .get(&(action, post_state))
            .ok_or(PgError::NoTransition)?;
        if self.eval_formula(guard) {
            for (var, effect) in self
                .effects
                .get(action.0)
                .expect("action has been validated before")
            {
                // Not using the 'Self::assign' method because:
                // - borrow checker
                // - effects are validated before, so no need to type-check again
                *self
                    .vars
                    .get_mut(var.0)
                    .expect("effect has been validated before") = self.eval(effect);
            }
            self.current_location = post_state;
            Ok(())
        } else {
            Err(PgError::UnsatisfiedGuard)
        }
    }

    pub(super) fn eval(&self, effect: &Expression) -> Val {
        match effect {
            Expression::Boolean(formula) => Val::Boolean(self.eval_formula(formula)),
            Expression::Integer(expr) => Val::Integer(self.eval_expr(expr)),
            Expression::Unit => Val::Unit,
        }
    }

    pub(super) fn assign(&mut self, var: Var, val: Val) -> Result<Val, PgError> {
        let var_content = self
            .vars
            .get_mut(var.0)
            .ok_or(PgError::NonExistingVar(var))?;
        match var_content {
            Val::Boolean(_) if !matches!(val, Val::Boolean(_)) => Err(PgError::Mismatched),
            Val::Integer(_) if !matches!(val, Val::Integer(_)) => Err(PgError::Mismatched),
            _ => {
                let previous_val = *var_content;
                *var_content = val;
                Ok(previous_val)
            }
        }
    }

    fn eval_formula(&self, formula: &Formula) -> bool {
        match formula {
            Formula::And(lhs, rhs) => self.eval_formula(lhs) && self.eval_formula(rhs),
            Formula::Or(lhs, rhs) => self.eval_formula(lhs) || self.eval_formula(rhs),
            Formula::Implies(lhs, rhs) => !self.eval_formula(lhs) || self.eval_formula(rhs),
            Formula::Not(subform) => !self.eval_formula(subform),
            Formula::Prop(var) => {
                let val = self
                    .vars
                    .get(var.0)
                    .expect("formula has been validated before");
                if let Val::Boolean(prop) = val {
                    *prop
                } else {
                    unreachable!("formula has been validated before");
                }
            }
            Formula::Equal(lhs, rhs) => self.eval_expr(lhs) == self.eval_expr(rhs),
            Formula::Less(lhs, rhs) => self.eval_expr(lhs) < self.eval_expr(rhs),
            Formula::LessEq(lhs, rhs) => self.eval_expr(lhs) <= self.eval_expr(rhs),
            Formula::True => true,
            Formula::False => false,
        }
    }

    fn eval_expr(&self, expr: &IntExpr) -> Integer {
        match expr {
            IntExpr::Const(int) => *int,
            IntExpr::Var(var) => {
                if let Val::Integer(val) = self
                    .vars
                    .get(var.0)
                    .expect("formula has been validated before")
                {
                    *val
                } else {
                    unreachable!("formula has been validated before");
                }
            }
            IntExpr::Opposite(expr) => -self.eval_expr(expr),
            IntExpr::Sum(lhs, rhs) => self.eval_expr(lhs) + self.eval_expr(rhs),
            IntExpr::Mult(lhs, rhs) => self.eval_expr(lhs) * self.eval_expr(rhs),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_graph() -> Result<(), PgError> {
        // Create Program Graph
        let mut builder = ProgramGraphBuilder::new();
        // Variables
        let battery = builder.new_var(VarType::Integer);
        // Locations
        let initial = builder.initial_location();
        let left = builder.new_location();
        let center = builder.new_location();
        let right = builder.new_location();
        // Actions
        let initialize = builder.new_action();
        builder.add_effect(initialize, battery, Expression::Integer(IntExpr::Const(3)))?;
        let move_left = builder.new_action();
        builder.add_effect(
            move_left,
            battery,
            Expression::Integer(IntExpr::Sum(
                Box::new(IntExpr::Var(battery)),
                Box::new(IntExpr::Opposite(Box::new(IntExpr::Const(1)))),
            )),
        )?;
        let move_right = builder.new_action();
        builder.add_effect(
            move_right,
            battery,
            Expression::Integer(IntExpr::Sum(
                Box::new(IntExpr::Var(battery)),
                Box::new(IntExpr::Opposite(Box::new(IntExpr::Const(1)))),
            )),
        )?;
        // Guards
        let out_of_charge = Formula::Less(IntExpr::Const(0), IntExpr::Var(battery));
        // Program graph definition
        builder.add_transition(initial, initialize, center, Formula::True)?;
        builder.add_transition(left, move_right, center, out_of_charge.clone())?;
        builder.add_transition(center, move_right, right, out_of_charge.clone())?;
        builder.add_transition(right, move_left, center, out_of_charge.clone())?;
        builder.add_transition(center, move_left, left, out_of_charge)?;
        // Execution
        let mut pg = builder.build();
        assert_eq!(pg.possible_transitions().len(), 1);
        pg.transition(initialize, center)?;
        assert_eq!(pg.possible_transitions().len(), 2);
        pg.transition(move_right, right)?;
        assert_eq!(pg.possible_transitions().len(), 1);
        pg.transition(move_right, right).expect_err("already right");
        assert_eq!(pg.possible_transitions().len(), 1);
        pg.transition(move_left, center)?;
        assert_eq!(pg.possible_transitions().len(), 2);
        pg.transition(move_left, left)?;
        assert_eq!(pg.possible_transitions().len(), 0);
        pg.transition(move_left, left).expect_err("battery = 0");
        Ok(())
    }
}
