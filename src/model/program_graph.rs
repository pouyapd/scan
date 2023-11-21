//! TODO list:
//! -[ ] use fast hasher for hashmap and hashset

use std::{collections::HashMap, rc::Rc};

use super::formula::*;

// Use of "Newtype" pattern to define different types of indexes.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Location(usize);

// Use of "Newtype" pattern to define different types of indexes.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Action(usize);

type Effect = Box<dyn Fn(&mut Eval)>;

type Transitions = Vec<HashMap<(Action, Location), Formula>>;

#[derive(Debug)]
pub enum PgErr {
    MissingAction(Action),
    MissingLocation(Location),
}

pub struct ProgramGraph {
    // `Hashmap`s (and thus `Eval`) is not hashable,
    // so we need to use a lambda to represent the inner function type.
    effects: Vec<Effect>,
    // We can assume there is at most one condition by logical disjunction
    transitions: Transitions,
}

impl Default for ProgramGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgramGraph {
    pub fn new() -> Self {
        Self {
            effects: Vec::new(),
            transitions: Vec::new(),
        }
    }

    pub fn new_action(&mut self, effect: impl Fn(&mut Eval) + 'static) -> Action {
        // Actions are indexed progressively
        let idx = self.effects.len();
        self.effects.push(Box::new(effect));
        Action(idx)
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
    ) -> Result<(), PgErr> {
        let _ = self
            .transitions
            .get_mut(pre.0)
            .ok_or(PgErr::MissingLocation(pre))?
            .entry((action, post))
            .and_modify(|previous_guard| {
                *previous_guard =
                    Formula::Or(Box::new(previous_guard.clone()), Box::new(guard.clone()));
            })
            .or_insert(guard);
        Ok(())
    }
}

#[derive(Debug)]
pub enum ExecutionErr {
    Type(TypeErr),
    NoTransition,
    UnsatisfiedGuard,
    UndefinedEffect,
}

pub struct Execution {
    current_location: Location,
    vars: Eval,
    pg: Rc<ProgramGraph>,
}

impl Execution {
    pub fn new(initial_location: Location, pg: Rc<ProgramGraph>) -> Self {
        Execution {
            current_location: initial_location,
            vars: Eval::new(),
            pg,
        }
    }

    pub fn possible_transitions(&self) -> Result<Vec<(Action, Location)>, TypeErr> {
        self.pg
            .transitions
            .get(self.current_location.0)
            .unwrap_or(&HashMap::new())
            .iter()
            .filter_map(|((action, post), guard)| match guard.eval(&self.vars) {
                Err(err) => Some(Err(err)),
                Ok(check) if check => Some(Ok((*action, *post))),
                _ => None,
            })
            .collect::<Result<Vec<_>, _>>()
    }

    pub fn transition(&mut self, action: Action, post_state: Location) -> Result<(), ExecutionErr> {
        let guard = self
            .pg
            .transitions
            .get(self.current_location.0)
            .ok_or(ExecutionErr::NoTransition)?
            .get(&(action, post_state))
            .ok_or(ExecutionErr::NoTransition)?;
        if guard.eval(&self.vars).map_err(ExecutionErr::Type)? {
            let effect = self
                .pg
                .effects
                .get(action.0)
                .ok_or(ExecutionErr::UndefinedEffect)?;
            effect(&mut self.vars);
            self.current_location = post_state;
            Ok(())
        } else {
            Err(ExecutionErr::UnsatisfiedGuard)
        }
    }
}
