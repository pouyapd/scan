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
//! let result = pg.transition(action, post_loc);
//!
//! // Performing a transition can fail, in particular, if the transition was not allowed
//! result.expect("The transition from the initial location onto itself is possible");
//!
//! // There are no more possible transitions
//! assert!(pg.possible_transitions().next().is_none());
//!
//! // Attempting to transition results in an error
//! pg.transition(action, post_loc).expect_err("The transition is not possible");
//! ```

mod builder;

use super::grammar::*;
use crate::Time;
pub use builder::*;
use core::panic;
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

/// Wait action to advance time.
/// Not to be used directly as an action or exposed to the user.
/// It cannot have effects.
const WAIT: Action = Action(u16::MAX - 1);

/// Reference clock of Program Graph.
/// Not to be reset or exposed to the user.
const TIME: Clock = Clock(0);

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
    /// Wait action should not be used directly.
    #[error("direct use of wait action")]
    Wait,
    /// Cannot reset global timer.
    #[error("cannot reset global timer")]
    TimeClock,
    /// A type error
    #[error("type error")]
    Type(#[source] TypeError),
}

#[derive(Debug)]
enum FnEffect {
    Effects(Vec<(Var, FnExpression<Var>)>),
    Send(FnExpression<Var>),
    Receive(Var),
}

type Transition = (Action, Location, Option<FnExpression<Var>>);

#[derive(Debug)]
struct ProgramGraphDef {
    effects: Vec<FnEffect>,
    transitions: Vec<Vec<Transition>>,
}

impl ProgramGraphDef {
    // Returns transition's guard.
    // Panics if the pre- or post-state do not exist.
    // Returns error if the transition does not exist.
    #[inline(always)]
    fn guard(
        &self,
        pre_state: Location,
        action: Action,
        post_state: Location,
    ) -> Result<Option<&FnExpression<Var>>, PgError> {
        let transitions = &self.transitions[pre_state.0 as usize];
        transitions
            .binary_search_by_key(&(action, post_state), |(a, p, _)| (*a, *p))
            .map(|guard_idx| transitions[guard_idx].2.as_ref())
            .map_err(|_| PgError::MissingTransition)
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
pub struct ProgramGraph {
    current_location: Location,
    vars: Vec<Val>,
    def: Arc<ProgramGraphDef>,
}

impl ProgramGraph {
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
    /// let mut pg = pg_builder.build();
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
    pub fn possible_transitions(&self) -> impl Iterator<Item = (Action, Location)> + '_ {
        self.def.transitions[self.current_location.0 as usize]
            .iter()
            .filter_map(|(action, post_state, guard)| {
                // WAIT should not be called directly!
                if *action == WAIT {
                    None
                } else if guard.as_ref().map_or(true, |guard| {
                    if let Val::Boolean(pass) = guard.eval(&|var| self.vars[var.0 as usize].clone())
                    {
                        pass
                    } else {
                        panic!("guard is not a boolean");
                    }
                }) {
                    if let Ok(Some(time_invariant)) = self.def.guard(*post_state, WAIT, *post_state)
                    {
                        // If action has effects
                        if *action != EPSILON {
                            if let FnEffect::Effects(ref effects) =
                                self.def.effects[action.0 as usize]
                            {
                                if !effects.is_empty() {
                                    // Avoid cloning variables unless it is absolutley necessary
                                    let mut vars = self.vars.clone();
                                    for (var, effect) in effects {
                                        vars[var.0 as usize] =
                                            effect.eval(&|var| vars[var.0 as usize].clone());
                                    }
                                    if let Val::Boolean(pass) = time_invariant
                                        .eval(&move |var| vars[var.0 as usize].clone())
                                    {
                                        return pass.then_some((*action, *post_state));
                                    } else {
                                        panic!("guard is not a boolean");
                                    }
                                }
                            }
                        }
                        // If action has no effects
                        if let Val::Boolean(pass) =
                            time_invariant.eval(&|var| self.vars[var.0 as usize].clone())
                        {
                            pass.then_some((*action, *post_state))
                        } else {
                            panic!("guard is not a boolean");
                        }
                    } else {
                        Some((*action, *post_state))
                    }
                } else {
                    None
                }
            })
    }

    #[inline(always)]
    fn satisfies_guard(&self, action: Action, post_state: Location) -> Result<bool, PgError> {
        let transitions = &self.def.transitions[self.current_location.0 as usize];
        transitions
            .binary_search_by_key(&(action, post_state), |(a, p, _)| (*a, *p))
            .map(|guard_idx| {
                if let Some(ref guard) = transitions[guard_idx].2 {
                    if let Val::Boolean(pass) = guard.eval(&|var| self.vars[var.0 as usize].clone())
                    {
                        pass
                    } else {
                        panic!("guard is not a boolean");
                    }
                } else {
                    true
                }
            })
            .map_err(|_| PgError::MissingTransition)
    }

    /// Executes a transition characterized by the argument action and post-state.
    ///
    /// Fails if the requested transition is not admissible,
    /// or if the post-location time invariants are violated.
    pub fn transition(&mut self, action: Action, post_state: Location) -> Result<(), PgError> {
        if action == WAIT {
            return Err(PgError::Wait);
        } else if !self.satisfies_guard(action, post_state)? {
            return Err(PgError::UnsatisfiedGuard);
        } else if let Ok(Some(time_invariant)) = self.def.guard(post_state, WAIT, post_state) {
            let mut backup = Vec::with_capacity(self.vars.len());
            // If action has effects
            if action != EPSILON {
                if let FnEffect::Effects(ref effects) = self.def.effects[action.0 as usize] {
                    if !effects.is_empty() {
                        // Clone only if necessary
                        backup = self.vars.clone();
                        for (var, effect) in effects {
                            self.vars[var.0 as usize] =
                                effect.eval(&|var| self.vars[var.0 as usize].clone());
                        }
                    }
                } else {
                    return Err(PgError::Communication(action));
                }
            }
            // Self::satisfies_guard should only be called after setting the post-location!
            if let Val::Boolean(pass) =
                time_invariant.eval(&|var| self.vars[var.0 as usize].clone())
            {
                if !pass {
                    // Backup is unused if empty
                    if action != EPSILON && !backup.is_empty() {
                        self.vars = backup;
                    }
                    return Err(PgError::UnsatisfiedGuard);
                }
            } else {
                panic!("guard is not a boolean");
            }
        } else if action != EPSILON {
            if let FnEffect::Effects(ref effects) = self.def.effects[action.0 as usize] {
                for (var, effect) in effects {
                    self.vars[var.0 as usize] =
                        effect.eval(&|var| self.vars[var.0 as usize].clone());
                }
            } else {
                return Err(PgError::Communication(action));
            }
        }
        self.current_location = post_state;
        Ok(())
    }

    /// Returns the current time of the Program Graph.
    #[inline(always)]
    pub fn time(&self) -> Time {
        if let Val::Integer(time) = self.vars[TIME.0 as usize] {
            time as Time
        } else {
            panic!("Time must be an Integer variable");
        }
    }

    /// Sets the time of the Program Graph.
    /// Only to be used by [`channel_system::ChannelSystem`] to recover from a failed wait
    /// (which can happen by violating some time invariant).
    #[inline(always)]
    pub(crate) fn set_time(&mut self, time: Time) {
        self.vars[TIME.0 as usize] = Val::Integer(time as Integer);
    }

    /// Waits a given amount of time-units.
    ///
    /// Returns error if the waiting would violate the current location's time invariant (if any).
    pub fn wait(&mut self, delta: Time) -> Result<(), PgError> {
        let prev_time;
        if let Val::Integer(ref mut time) = self.vars[TIME.0 as usize] {
            prev_time = *time;
            *time += delta as Integer;
        } else {
            panic!("Time must be an Integer variable");
        }
        let transitions = &self.def.transitions[self.current_location.0 as usize];
        if transitions
            .binary_search_by_key(&(WAIT, self.current_location), |(a, p, _)| (*a, *p))
            .map(|guard_idx| {
                if let Some(ref guard) = transitions[guard_idx].2 {
                    if let Val::Boolean(pass) = guard.eval(&|var| self.vars[var.0 as usize].clone())
                    {
                        pass
                    } else {
                        panic!("guard is not a boolean");
                    }
                } else {
                    true
                }
            })
            .unwrap_or(true)
        {
            Ok(())
        } else {
            // If the location's invariant is not satisfied,
            // reset time to original value.
            self.vars[TIME.0 as usize] = Val::Integer(prev_time);
            Err(PgError::UnsatisfiedGuard)
        }
    }

    pub(crate) fn send(&mut self, action: Action, post_state: Location) -> Result<Val, PgError> {
        if !self.satisfies_guard(action, post_state)? {
            Err(PgError::UnsatisfiedGuard)
        } else if let FnEffect::Send(effect) = &self.def.effects[action.0 as usize] {
            let val = effect.eval(&|var| self.vars[var.0 as usize].clone());
            self.current_location = post_state;
            Ok(val)
        } else {
            Err(PgError::NotSend(action))
        }
    }

    pub(crate) fn receive(
        &mut self,
        action: Action,
        post_state: Location,
        val: Val,
    ) -> Result<(), PgError> {
        if !self.satisfies_guard(action, post_state)? {
            Err(PgError::UnsatisfiedGuard)
        } else if let FnEffect::Receive(var) = self.def.effects[action.0 as usize] {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait() {
        let builder = ProgramGraphBuilder::new();
        let initial = builder.initial_location();
        let mut pg = builder.build();
        assert_eq!(pg.possible_transitions().count(), 0);
        pg.transition(WAIT, initial).expect_err("forbidden");
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
        pg.transition(action, r#final).expect("transition to final");
        assert_eq!(pg.possible_transitions().count(), 0);
    }

    #[test]
    fn program_graph() -> Result<(), PgError> {
        // Create Program Graph
        let mut builder = ProgramGraphBuilder::new();
        // Variables
        let battery = builder.new_var(Expression::Const(Val::Integer(0)))?;
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
        pg.transition(initialize, center).expect("initialize");
        assert_eq!(pg.possible_transitions().count(), 2);
        pg.transition(move_right, right).expect("move right");
        assert_eq!(pg.possible_transitions().count(), 1);
        pg.transition(move_right, right).expect_err("already right");
        assert_eq!(pg.possible_transitions().count(), 1);
        pg.transition(move_left, center).expect("move left");
        assert_eq!(pg.possible_transitions().count(), 2);
        pg.transition(move_left, left).expect("move left");
        assert_eq!(pg.possible_transitions().count(), 0);
        pg.transition(move_left, left).expect_err("battery = 0");
        Ok(())
    }
}
