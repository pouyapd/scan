//! TODO list:
//! -[ ] use fast hasher for hashmap and hashset

use std::{collections::HashMap, rc::Rc};

use super::formula::*;

pub type Effects = HashMap<Action, Box<dyn Fn(&mut Eval)>>;

pub type Transitions = HashMap<Location, HashMap<(Action, Location), Formula>>;

pub struct ProgramGraph {
    // `Hashmap`s (and thus `Eval`) is not hashable,
    // so we need to use a lambda to represent the inner function type.
    effects: Effects,
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
            effects: HashMap::new(),
            transitions: HashMap::new(),
        }
    }

    pub fn add_effect(&mut self, action: Action, effect: impl Fn(&mut Eval) + 'static) {
        self.effects.insert(action, Box::new(effect));
    }

    pub fn with_effect(mut self, action: Action, effect: impl Fn(&mut Eval) + 'static) -> Self {
        self.add_effect(action, Box::new(effect));
        self
    }

    pub fn add_transition(
        &mut self,
        pre: Location,
        action: Action,
        post: Location,
        guard: Formula,
    ) {
        let map = self.transitions.entry(pre).or_default();
        let _ = map.insert((action, post), guard);
    }

    pub fn with_transition(
        mut self,
        pre: Location,
        action: Action,
        post: Location,
        guard: Formula,
    ) -> Self {
        self.add_transition(pre, action, post, guard);
        self
    }

    // pub fn build(self) -> ProgramGraph {
    //     ProgramGraph {
    //         // current_location: initial_location,
    //         // vars: Eval::new(),
    //         effects: self.effects,
    //         transitions: self.transitions,
    //     }
    // }
}

// Use of "Newtype" pattern to define different types of indexes.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Location(pub usize);

// Use of "Newtype" pattern to define different types of indexes.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Action(pub usize);

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
            .get(&self.current_location)
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
            .get(&self.current_location)
            .ok_or(ExecutionErr::NoTransition)?
            .get(&(action, post_state))
            .ok_or(ExecutionErr::NoTransition)?;
        if guard.eval(&self.vars).map_err(ExecutionErr::Type)? {
            let effect = self
                .pg
                .effects
                .get(&action)
                .ok_or(ExecutionErr::UndefinedEffect)?;
            effect(&mut self.vars);
            self.current_location = post_state;
            Ok(())
        } else {
            Err(ExecutionErr::UnsatisfiedGuard)
        }
    }
}
