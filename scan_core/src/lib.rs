//! Implementation of *program graphs* (PG) and *channel systems* (CS) formalisms[^1]
//! for use in the SCAN model checker.
//!
//! [^1]: Baier, C., & Katoen, J. (2008). *Principles of model checking*. MIT Press.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub mod channel_system;
mod grammar;
mod model;
mod pmtl;
pub mod program_graph;
mod smc;

pub use grammar::*;
pub use model::*;
pub use pmtl::*;
pub use smc::*;

/// The type that represents time.
pub type Time = u32;

/// Trait that handles streaming of traces,
/// e.g., to print them to file.
pub trait Tracer<A> {
    /// Initialize new streaming.
    ///
    /// This method needs to be called once, before calls to [`Self::trace`].
    fn init(&mut self);

    /// Stream a new state of the trace.
    fn trace<I: IntoIterator<Item = Val>>(&mut self, action: &A, time: Time, ports: I);

    /// Finalize and close streaming.
    ///
    /// This method needs to be called at the end of the execution.
    fn finalize(self, success: Option<bool>, violation: Option<String>);
}
