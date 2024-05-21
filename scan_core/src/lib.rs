//! Implementation of *program graphs* (PG) and *channel systems* (CS) formalisms
//! for use in the SCAN model checker.

pub mod channel_system;
mod grammar;
pub mod program_graph;

pub use grammar::*;
