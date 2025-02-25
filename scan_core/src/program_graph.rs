//! Implementation of the PG model of computation.
//!
//! A _Program Graph_ is given by:
//!
//! - a finite set `L` of _locations_;
//! - a finite set `A` of _actions_;
//! - a finite set `V` of _typed variebles_;
//! - a _transition relation_ that associates pairs of locations (pre-location and post-location) and an action with a Boolean expression (the _guard_ of the transition);
//! - for each actions, a set of _effects_, i.e., a variabe `x` from `V` and an expression in the variables of `V` of the same type as `x`.
//!
//! The state of a PG is given by its current location and the value assigned to each variable.
//! The PG's state evolves by non-deterministically choosing a transition whose pre-state is the current state,
//! and whose guard expression evaluates to `true`.
//! Then, the post-state of the chosen transition becomes the current state of the PG.
//! Finally, the effects of the transition's associated action are applied in order,
//! by assigning each effect's variable the value of the effect's expression evaluation.
//!
//! A PG is represented by a [`ProgramGraph`] and defined through a [`ProgramGraphBuilder`],
//! by adding, one at a time, new locations, actions, effects, guards and transitions.
//! Then, the [`ProgramGraph`] is built from the [`ProgramGraphBuilder`]
//! and can be executed by performing transitions,
//! though the structure of the PG itself can no longer be altered.
//!
//! ```
//! # use scan_core::program_graph::ProgramGraphBuilder;
//! // Create a new PG builder
//! let mut pg_builder = ProgramGraphBuilder::new();
//!
//! // The builder is initialized with an initial location
//! let initial_loc = pg_builder.initial_location();
//!
//! // Create a new action
//! let action = pg_builder.new_action();
//!
//! // Create a new location
//! let post_loc = pg_builder.new_location();
//!
//! // Add a transition (the guard is optional, and None is equivalent to the guard always being true)
//! let result = pg_builder.add_transition(initial_loc, action, post_loc, None);
//!
//! // This can only fail if the builder does not recognize either the locations
//! // or the action defining the transition
//! result.expect("both the initial location and the action belong to the PG");
//!
//! // Build the PG from its builder
//! // The builder is always guaranteed to build a well-defined PG and building cannot fail
//! let mut pg = pg_builder.build();
//!
//! // Execution starts in the initial location
//! assert_eq!(pg.current_location(), initial_loc);
//!
//! // Compute the possible transitions on the PG
//! assert_eq!(Vec::from_iter(pg.possible_transitions()), vec![(action, post_loc)]);
//!
//! // Perform a transition
//! # use rand::{Rng, SeedableRng};
//! # use rand::rngs::SmallRng;
//! let mut rng = SmallRng::from_os_rng();
//! let result = pg.transition(action, post_loc, &mut rng);
//!
//! // Performing a transition can fail, in particular, if the transition was not allowed
//! result.expect("The transition from the initial location onto itself is possible");
//!
//! // There are no more possible transitions
//! assert!(pg.possible_transitions().next().is_none());
//!
//! // Attempting to transition results in an error
//! pg.transition(action, post_loc, &mut rng).expect_err("The transition is not possible");
//! ```

mod builder;

use super::grammar::*;
use crate::Time;
pub use builder::*;
use core::panic;
use rand::{rngs::mock::StepRng, Rng};
use std::sync::Arc;
use thiserror::Error;

/// An indexing object for locations in a PG.
///
/// These cannot be directly created or manipulated,
/// but have to be generated and/or provided by a [`ProgramGraphBuilder`] or [`ProgramGraph`].
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Location(u16);

/// An indexing object for actions in a PG.
///
/// These cannot be directly created or manipulated,
/// but have to be generated and/or provided by a [`ProgramGraphBuilder`] or [`ProgramGraph`].
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Action(u16);

/// Epsilon action to enable autonomous transitions.
/// It cannot have effects.
const EPSILON: Action = Action(u16::MAX);

/// An indexing object for typed variables in a PG.
///
/// These cannot be directly created or manipulated,
/// but have to be generated and/or provided by a [`ProgramGraphBuilder`] or [`ProgramGraph`].
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Var(u16);

/// An indexing object for clocks in a PG.
///
/// These cannot be directly created or manipulated,
/// but have to be generated and/or provided by a [`ProgramGraphBuilder`] or [`ProgramGraph`].
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Clock(u16);

/// A time constraint given by a clock and, optionally, a lower bound and/or an upper bound.
pub type TimeConstraint = (Clock, Option<Time>, Option<Time>);

/// An expression using PG's [`Var`] as variables.
pub type PgExpression = Expression<Var>;

/// The error type for operations with [`ProgramGraphBuilder`]s and [`ProgramGraph`]s.
#[derive(Debug, Clone, Error)]
pub enum PgError {
    /// The expression is badly typed.
    #[error("malformed expression {0:?}")]
    BadExpression(PgExpression),
    /// There is no such action in the PG.
    #[error("action {0:?} does not belong to this program graph")]
    MissingAction(Action),
    /// There is no such clock in the PG.
    #[error("clock {0:?} does not belong to this program graph")]
    MissingClock(Clock),
    /// There is no such location in the PG.
    #[error("location {0:?} does not belong to this program graph")]
    MissingLocation(Location),
    /// There is no such variable in the PG.
    #[error("location {0:?} does not belong to this program graph")]
    MissingVar(Var),
    /// The PG does not allow this transition.
    #[error("there is no such transition")]
    MissingTransition,
    /// Types that should be matching are not,
    /// or are not compatible with each other.
    #[error("type mismatch")]
    TypeMismatch,
    /// Transition's guard is not satisfied.
    #[error("the guard has not been satisfied")]
    UnsatisfiedGuard,
    /// The tuple has no component for such index.
    #[error("the tuple has no {0} component")]
    MissingComponent(usize),
    /// Cannot add effects to a Receive action.
    #[error("cannot add effects to a Receive action")]
    EffectOnReceive,
    /// Cannot add effects to a Send action.
    #[error("cannot add effects to a Send action")]
    EffectOnSend,
    /// This action is a communication (either Send or Receive).
    #[error("{0:?} is a communication (either Send or Receive)")]
    Communication(Action),
    /// The action is a not a Send communication.
    #[error("{0:?} is a not a Send communication")]
    NotSend(Action),
    /// The action is a not a Receive communication.
    #[error("{0:?} is a not a Receive communication")]
    NotReceive(Action),
    /// The epsilon action has no effects.
    #[error("The epsilon action has no effects")]
    NoEffects,
    /// A time invariant is not satisfied.
    #[error("A time invariant is not satisfied")]
    Invariant,
    /// A type error
    #[error("type error")]
    Type(#[source] TypeError),
}

#[derive(Debug)]
enum FnEffect<R: Rng> {
    // NOTE: Could use a SmallVec for clock resets
    Effects(Vec<(Var, FnExpression<Var, R>)>, Vec<Clock>),
    Send(FnExpression<Var, R>),
    Receive(Var),
}

type Guard = FnExpression<Var, StepRng>;

type Transition = (Action, Location, Option<Guard>, Vec<TimeConstraint>);

#[derive(Debug)]
struct ProgramGraphDef<R: Rng> {
    effects: Vec<FnEffect<R>>,
    locations: Vec<(Vec<Transition>, Vec<TimeConstraint>)>,
}

impl<R: Rng> ProgramGraphDef<R> {
    // Returns transition's guard.
    // Panics if the pre- or post-state do not exist.
    // Returns error if the transition does not exist.
    #[inline(always)]
    fn guards(
        &self,
        pre_state: Location,
        action: Action,
        post_state: Location,
    ) -> impl Iterator<Item = (&Option<Guard>, &Vec<TimeConstraint>)> {
        let (transitions, _) = &self.locations[pre_state.0 as usize];
        let part = transitions.partition_point(|(a, p, ..)| (*a, *p) < (action, post_state));
        transitions[part..]
            .iter()
            .take_while(move |(a, p, ..)| (*a, *p) == (action, post_state))
            .map(|(_, _, g, c)| (g, c))
    }
}

/// Representation of a PG that can be executed transition-by-transition.
///
/// The structure of the PG cannot be changed,
/// meaning that it is not possible to introduce new locations, actions, variables, etc.
/// Though, this restriction makes it so that cloning the [`ProgramGraph`] is cheap,
/// because only the internal state needs to be duplicated.
///
/// The only way to produce a [`ProgramGraph`] is through a [`ProgramGraphBuilder`].
/// This guarantees that there are no type errors involved in the definition of action's effects and transitions' guards,
/// and thus the PG will always be in a consistent state.
#[derive(Clone, Debug)]
pub struct ProgramGraph<R: Rng> {
    current_location: Location,
    vars: Vec<Val>,
    clocks: Vec<Time>,
    def: Arc<ProgramGraphDef<R>>,
}

impl<R: Rng> ProgramGraph<R> {
    /// Returns the current location.
    ///
    /// ```
    /// # use scan_core::program_graph::ProgramGraphBuilder;
    /// // Create a new PG builder
    /// let mut pg_builder = ProgramGraphBuilder::new();
    ///
    /// // The builder is initialized with an initial location
    /// let initial_loc = pg_builder.initial_location();
    ///
    /// // Build the PG from its builder
    /// // The builder is always guaranteed to build a well-defined PG and building cannot fail
    /// # use rand::rngs::SmallRng;
    /// let mut pg = pg_builder.build::<SmallRng>();
    ///
    /// // Execution starts in the initial location
    /// assert_eq!(pg.current_location(), initial_loc);
    /// ```
    #[inline(always)]
    pub fn current_location(&self) -> Location {
        self.current_location
    }

    /// Iterates over all transitions that can be admitted in the current state.
    ///
    /// An admittable transition is characterized by the required action and the post-state
    /// (the pre-state being necessarily the current state of the machine).
    /// The guard (if any) is guaranteed to be satisfied.
    pub fn possible_transitions(&self) -> impl Iterator<Item = (Action, Location)> + use<'_, R> {
        self.def.locations[self.current_location.0 as usize]
            .0
            .iter()
            .filter_map(|(action, post_state, guard, constraints)| {
                let (_, ref invariants) = self.def.locations[post_state.0 as usize];
                let resets = if *action == EPSILON {
                    &Vec::new()
                } else {
                    match self.def.effects[action.0 as usize] {
                        FnEffect::Effects(_, ref resets) => resets,
                        FnEffect::Send(_) | FnEffect::Receive(_) => &Vec::new(),
                    }
                };
                self.active_transition(guard.as_ref(), constraints, invariants, resets)
                    .then_some((*action, *post_state))
            })
    }

    #[inline(always)]
    fn active_transition(
        &self,
        guard: Option<&Guard>,
        constraints: &[TimeConstraint],
        invariants: &[TimeConstraint],
        resets: &[Clock],
    ) -> bool {
        guard.is_none_or(|guard| {
            // TODO FIXME: is there a way to avoid creating a dummy RNG?
            let mut rng = StepRng::new(0, 0);
            if let Val::Boolean(pass) =
                guard.eval(&|var| self.vars[var.0 as usize].clone(), &mut rng)
            {
                pass
            } else {
                panic!("guard is not a boolean");
            }
        }) && constraints.iter().all(|(c, l, u)| {
            let time = self.clocks[c.0 as usize];
            l.is_none_or(|l| l <= time) && u.is_none_or(|u| time < u)
        }) && invariants.iter().all(|(c, l, u)| {
            // TODO NOTE: use binary search on resets?
            let time = if resets.contains(c) {
                0
            } else {
                self.clocks[c.0 as usize]
            };
            l.is_none_or(|l| l <= time) && u.is_none_or(|u| time < u)
        })
    }

    /// Executes a transition characterized by the argument action and post-state.
    ///
    /// Fails if the requested transition is not admissible,
    /// or if the post-location time invariants are violated.
    pub fn transition<'a>(
        &'a mut self,
        action: Action,
        post_state: Location,
        rng: &'a mut R,
    ) -> Result<(), PgError> {
        let (_, ref invariants) = self.def.locations[post_state.0 as usize];
        let (effects, resets) = if action == EPSILON {
            (&Vec::new(), &Vec::new())
        } else {
            match self.def.effects[action.0 as usize] {
                FnEffect::Effects(ref effects, ref resets) => (effects, resets),
                FnEffect::Send(_) | FnEffect::Receive(_) => {
                    return Err(PgError::Communication(action))
                }
            }
        };
        if self
            .def
            .guards(self.current_location, action, post_state)
            .any(|(guard, constraints)| {
                self.active_transition(guard.as_ref(), constraints, invariants, resets)
            })
        {
            for (var, effect) in effects {
                self.vars[var.0 as usize] =
                    effect.eval(&|var| self.vars[var.0 as usize].clone(), rng);
            }
            for clock in resets {
                self.clocks[clock.0 as usize] = 0;
            }
            self.current_location = post_state;
            Ok(())
        } else {
            Err(PgError::UnsatisfiedGuard)
        }
    }

    /// Checks if it is possible to wait a given amount of time-units without violating the time invariants.
    pub fn can_wait(&self, delta: Time) -> bool {
        let (_, ref invariants) = self.def.locations[self.current_location.0 as usize];
        invariants.iter().all(|(c, l, u)| {
            // Invariants need to be satisfied during the whole wait.
            let start_time = self.clocks[c.0 as usize];
            let end_time = start_time + delta;
            l.is_none_or(|l| l <= start_time) && u.is_none_or(|u| end_time < u)
        })
    }

    /// Waits a given amount of time-units.
    ///
    /// Returns error if the waiting would violate the current location's time invariant (if any).
    pub fn wait(&mut self, delta: Time) -> Result<(), PgError> {
        if self.can_wait(delta) {
            self.clocks.iter_mut().for_each(|t| *t += delta);
            Ok(())
        } else {
            Err(PgError::Invariant)
        }
    }

    pub(crate) fn send<'a>(
        &'a mut self,
        action: Action,
        post_state: Location,
        rng: &'a mut R,
    ) -> Result<Val, PgError> {
        let (_, ref invariants) = self.def.locations[post_state.0 as usize];
        if action == EPSILON {
            Err(PgError::NotSend(action))
        } else if self
            .def
            .guards(self.current_location, action, post_state)
            .any(|(guard, constraints)| {
                self.active_transition(guard.as_ref(), constraints, invariants, &[])
            })
        {
            if let FnEffect::Send(effect) = &self.def.effects[action.0 as usize] {
                let val = effect.eval(&|var| self.vars[var.0 as usize].clone(), rng);
                self.current_location = post_state;
                Ok(val)
            } else {
                Err(PgError::NotSend(action))
            }
        } else {
            Err(PgError::UnsatisfiedGuard)
        }
    }

    pub(crate) fn receive(
        &mut self,
        action: Action,
        post_state: Location,
        val: Val,
    ) -> Result<(), PgError> {
        let (_, ref invariants) = self.def.locations[post_state.0 as usize];
        if action == EPSILON {
            Err(PgError::NotSend(action))
        } else if self
            .def
            .guards(self.current_location, action, post_state)
            .any(|(guard, constraints)| {
                self.active_transition(guard.as_ref(), constraints, invariants, &[])
            })
        {
            if let FnEffect::Receive(var) = self.def.effects[action.0 as usize] {
                let var_content = self.vars.get_mut(var.0 as usize).expect("variable exists");
                if var_content.r#type() == val.r#type() {
                    *var_content = val;
                    self.current_location = post_state;
                    Ok(())
                } else {
                    Err(PgError::TypeMismatch)
                }
            } else {
                Err(PgError::NotReceive(action))
            }
        } else {
            Err(PgError::UnsatisfiedGuard)
        }
    }
}

#[cfg(test)]
mod tests {
    use rand::rngs::mock::StepRng;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    use super::*;

    #[test]
    fn wait() {
        let builder = ProgramGraphBuilder::new();
        let mut pg = builder.build::<SmallRng>();
        assert_eq!(pg.possible_transitions().count(), 0);
        pg.wait(1).expect("wait 1 time unit");
    }

    #[test]
    fn transitions() {
        let mut builder = ProgramGraphBuilder::new();
        let initial = builder.initial_location();
        let r#final = builder.new_location();
        let action = builder.new_action();
        builder
            .add_transition(initial, action, r#final, None)
            .expect("add transition");
        let mut pg = builder.build();
        assert_eq!(
            pg.possible_transitions().collect::<Vec<_>>(),
            vec![(action, r#final)]
        );
        let mut rng = SmallRng::from_seed([0; 32]);
        pg.transition(action, r#final, &mut rng)
            .expect("transition to final");
        assert_eq!(pg.possible_transitions().count(), 0);
    }

    #[test]
    fn program_graph() -> Result<(), PgError> {
        // Create Program Graph
        let mut builder = ProgramGraphBuilder::new();
        let mut rng = StepRng::new(0, 1);
        // Variables
        let battery = builder.new_var_with_rng(Expression::Const(Val::Integer(0)), &mut rng)?;
        // Locations
        let initial = builder.initial_location();
        let left = builder.new_location();
        let center = builder.new_location();
        let right = builder.new_location();
        // Actions
        let initialize = builder.new_action();
        builder.add_effect(initialize, battery, PgExpression::Const(Val::Integer(3)))?;
        let move_left = builder.new_action();
        builder.add_effect(
            move_left,
            battery,
            PgExpression::Sum(vec![
                PgExpression::Var(battery, Type::Integer),
                PgExpression::Const(Val::Integer(-1)),
            ]),
        )?;
        let move_right = builder.new_action();
        builder.add_effect(
            move_right,
            battery,
            PgExpression::Sum(vec![
                PgExpression::Var(battery, Type::Integer),
                PgExpression::Const(Val::Integer(-1)),
            ]),
        )?;
        // Guards
        let out_of_charge = PgExpression::Greater(Box::new((
            PgExpression::Var(battery, Type::Integer),
            PgExpression::Const(Val::Integer(0)),
        )));
        // Program graph definition
        builder.add_transition(initial, initialize, center, None)?;
        builder.add_transition(left, move_right, center, Some(out_of_charge.clone()))?;
        builder.add_transition(center, move_right, right, Some(out_of_charge.clone()))?;
        builder.add_transition(right, move_left, center, Some(out_of_charge.clone()))?;
        builder.add_transition(center, move_left, left, Some(out_of_charge))?;
        // Execution
        let mut pg = builder.build();
        assert_eq!(pg.possible_transitions().count(), 1);
        let mut rng = SmallRng::from_seed([0; 32]);
        pg.transition(initialize, center, &mut rng)
            .expect("initialize");
        assert_eq!(pg.possible_transitions().count(), 2);
        pg.transition(move_right, right, &mut rng)
            .expect("move right");
        assert_eq!(pg.possible_transitions().count(), 1);
        pg.transition(move_right, right, &mut rng)
            .expect_err("already right");
        assert_eq!(pg.possible_transitions().count(), 1);
        pg.transition(move_left, center, &mut rng)
            .expect("move left");
        assert_eq!(pg.possible_transitions().count(), 2);
        pg.transition(move_left, left, &mut rng).expect("move left");
        assert_eq!(pg.possible_transitions().count(), 0);
        pg.transition(move_left, left, &mut rng)
            .expect_err("battery = 0");
        Ok(())
    }
}
