//! Implementation of *program graphs* (PG) and *channel systems* (CS) formalisms
//! for use in the SCAN model checker.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub mod channel_system;
mod grammar;
pub mod program_graph;

pub use grammar::*;
