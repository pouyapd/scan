//! Parser and model builder for SCAN's XML specification format.

mod cs_builder;
mod parser;

pub use cs_builder::Sc2CsVisitor;
pub use parser::Parser;
